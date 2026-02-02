use dashmap::DashMap;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use helix_db::helixc::analyzer::{analyze, diagnostic::DiagnosticSeverity as HelixSeverity};
use helix_db::helixc::parser::types::{
    Content, ExpressionType, FieldType, HxFile, Source, StatementType, StepType,
};
use helix_db::helixc::parser::HelixParser;

/// Parse line and column from pest error messages like "--> 19:1"
fn parse_error_location(error_msg: &str) -> (u32, u32) {
    // Look for pattern like "--> 19:1" or "at line 19, column 1"
    if let Some(arrow_idx) = error_msg.find("--> ") {
        let after_arrow = &error_msg[arrow_idx + 4..];
        if let Some(newline_idx) = after_arrow.find('\n') {
            let loc_str = &after_arrow[..newline_idx];
            let parts: Vec<&str> = loc_str.split(':').collect();
            if parts.len() >= 2 {
                if let (Ok(line), Ok(col)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    return (line.saturating_sub(1), col.saturating_sub(1));
                }
            }
        }
    }
    (0, 0)
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: DashMap<Url, String>,
    parsed_cache: DashMap<String, Source>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            parsed_cache: DashMap::new(),
        }
    }

    /// Get all .hx/.hql files in the same directory as the given file
    fn get_sibling_files(&self, uri: &Url) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        if let Ok(path) = uri.to_file_path() {
            if let Some(parent) = path.parent() {
                if let Ok(entries) = fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if let Some(ext) = p.extension() {
                            if ext == "hx" || ext == "hql" {
                                files.push(p);
                            }
                        }
                    }
                }
            }
        }
        files
    }

    /// Parse all files in the directory and run analysis
    async fn analyze_workspace(&self, uri: &Url) {
        let files = self.get_sibling_files(uri);
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))
            .unwrap_or_default();

        let mut all_diagnostics: HashMap<Url, Vec<Diagnostic>> = HashMap::new();

        // Build Content structure with all files
        let mut hx_files = Vec::new();
        for file_path in &files {
            let content = if let Ok(file_uri) = Url::from_file_path(file_path) {
                if let Some(doc) = self.documents.get(&file_uri) {
                    doc.clone()
                } else {
                    fs::read_to_string(file_path).unwrap_or_default()
                }
            } else {
                fs::read_to_string(file_path).unwrap_or_default()
            };

            let file_name = file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            hx_files.push(HxFile {
                name: file_name,
                content,
            });
        }

        let content = Content {
            content: String::new(),
            files: hx_files,
            source: Source::default(),
        };

        // Parse and analyze
        match HelixParser::parse_source(&content) {
            Ok(source) => {
                // Run analyzer
                match analyze(&source) {
                    Ok((diagnostics, _)) => {
                        for diag in diagnostics {
                            // Find file path for this diagnostic
                            let file_name = diag
                                .location
                                .filepath
                                .clone()
                                .unwrap_or_default();

                            // Find the file path that matches
                            for file_path in &files {
                                let name = file_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();

                                if name == file_name || file_name.is_empty() {
                                    if let Ok(file_uri) = Url::from_file_path(file_path) {
                                        let severity = match diag.severity {
                                            HelixSeverity::Error => DiagnosticSeverity::ERROR,
                                            HelixSeverity::Warning => DiagnosticSeverity::WARNING,
                                            HelixSeverity::Info => DiagnosticSeverity::INFORMATION,
                                            HelixSeverity::Hint => DiagnosticSeverity::HINT,
                                            HelixSeverity::Empty => DiagnosticSeverity::INFORMATION,
                                        };

                                        let lsp_diag = Diagnostic {
                                            range: Range {
                                                start: Position {
                                                    line: diag.location.start.line.saturating_sub(1)
                                                        as u32,
                                                    character: diag
                                                        .location
                                                        .start
                                                        .column
                                                        .saturating_sub(1)
                                                        as u32,
                                                },
                                                end: Position {
                                                    line: diag.location.end.line.saturating_sub(1)
                                                        as u32,
                                                    character: diag
                                                        .location
                                                        .end
                                                        .column
                                                        .saturating_sub(1)
                                                        as u32,
                                                },
                                            },
                                            severity: Some(severity),
                                            source: Some("helixql".to_string()),
                                            message: diag.message.clone(),
                                            ..Default::default()
                                        };
                                        all_diagnostics
                                            .entry(file_uri)
                                            .or_default()
                                            .push(lsp_diag);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Analyzer error - try to extract location from error message
                        let error_msg = format!("{}", e);
                        let (line, col) = parse_error_location(&error_msg);

                        if let Some(file_path) = files.first() {
                            if let Ok(file_uri) = Url::from_file_path(file_path) {
                                let diag = Diagnostic {
                                    range: Range {
                                        start: Position {
                                            line,
                                            character: col,
                                        },
                                        end: Position {
                                            line,
                                            character: col + 50,
                                        },
                                    },
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    source: Some("helixql".to_string()),
                                    message: format!("Analyzer error: {}", e),
                                    ..Default::default()
                                };
                                all_diagnostics
                                    .entry(file_uri)
                                    .or_default()
                                    .push(diag);
                            }
                        }
                    }
                }

                // Cache the parsed source
                self.parsed_cache.insert(dir_key.clone(), source);
            }
            Err(e) => {
                // Parse error - extract location from error message
                let error_msg = format!("{}", e);
                let (line, col) = parse_error_location(&error_msg);

                if let Some(file_path) = files.first() {
                    if let Ok(file_uri) = Url::from_file_path(file_path) {
                        let diag = Diagnostic {
                            range: Range {
                                start: Position {
                                    line,
                                    character: col,
                                },
                                end: Position {
                                    line,
                                    character: col + 50,
                                },
                            },
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("helixql".to_string()),
                            message: error_msg,
                            ..Default::default()
                        };
                        all_diagnostics
                            .entry(file_uri)
                            .or_default()
                            .push(diag);
                    }
                }
            }
        }

        // Publish diagnostics for all files
        for file_path in &files {
            if let Ok(file_uri) = Url::from_file_path(file_path) {
                let diagnostics = all_diagnostics.remove(&file_uri).unwrap_or_default();
                self.client
                    .publish_diagnostics(file_uri, diagnostics, None)
                    .await;
            }
        }
    }

    /// Get word at position in document
    fn get_word_at_position(&self, uri: &Url, position: Position) -> Option<String> {
        let doc = self.documents.get(uri)?;
        let lines: Vec<&str> = doc.lines().collect();
        let line = lines.get(position.line as usize)?;
        let char_idx = position.character as usize;

        if char_idx > line.len() {
            return None;
        }

        // Find word boundaries
        let chars: Vec<char> = line.chars().collect();
        let mut start = char_idx;
        let mut end = char_idx;

        // Go backwards to find start
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }

        // Go forwards to find end
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }

        if start == end {
            return None;
        }

        Some(chars[start..end].iter().collect())
    }

    /// Get schema type hover info (for Node, Edge, or Vector types)
    fn get_type_hover_info(&self, uri: &Url, word: &str) -> Option<String> {
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))?;

        let source = self.parsed_cache.get(&dir_key)?;

        // Check all schema versions
        for (_version, schema) in &source.schema {
            // Check node definitions
            for node in &schema.node_schemas {
                if node.name.1 == word {
                    let mut fields_str = String::new();
                    for field in &node.fields {
                        let type_str = Self::field_type_to_string(&field.field_type);
                        fields_str.push_str(&format!("    {}: {}\n", field.name, type_str));
                    }
                    return Some(format!(
                        "**{}** (Node)\n\n```hql\nN::{} {{\n{}}}\n```",
                        word, word, fields_str
                    ));
                }
            }

            // Check edge definitions
            for edge in &schema.edge_schemas {
                if edge.name.1 == word {
                    let mut content = format!("    From: {}\n    To: {}\n", edge.from.1, edge.to.1);
                    if let Some(props) = &edge.properties {
                        content.push_str("    Properties {\n");
                        for field in props {
                            let type_str = Self::field_type_to_string(&field.field_type);
                            content.push_str(&format!("        {}: {}\n", field.name, type_str));
                        }
                        content.push_str("    }\n");
                    }
                    return Some(format!(
                        "**{}** (Edge)\n\n```hql\nE::{} {{\n{}}}\n```",
                        word, word, content
                    ));
                }
            }

            // Check vector definitions
            for vector in &schema.vector_schemas {
                if vector.name == word {
                    let mut fields_str = String::new();
                    for field in &vector.fields {
                        let type_str = Self::field_type_to_string(&field.field_type);
                        fields_str.push_str(&format!("    {}: {}\n", field.name, type_str));
                    }
                    return Some(format!(
                        "**{}** (Vector)\n\n```hql\nV::{} {{\n{}}}\n```",
                        word, word, fields_str
                    ));
                }
            }
        }

        None
    }

    /// Convert FieldType to a string representation
    fn field_type_to_string(ft: &FieldType) -> String {
        match ft {
            FieldType::String => "String".to_string(),
            FieldType::F32 => "F32".to_string(),
            FieldType::F64 => "F64".to_string(),
            FieldType::I8 => "I8".to_string(),
            FieldType::I16 => "I16".to_string(),
            FieldType::I32 => "I32".to_string(),
            FieldType::I64 => "I64".to_string(),
            FieldType::U8 => "U8".to_string(),
            FieldType::U16 => "U16".to_string(),
            FieldType::U32 => "U32".to_string(),
            FieldType::U64 => "U64".to_string(),
            FieldType::U128 => "U128".to_string(),
            FieldType::Boolean => "Boolean".to_string(),
            FieldType::Uuid => "Uuid".to_string(),
            FieldType::Date => "Date".to_string(),
            FieldType::Array(inner) => format!("[{}]", Self::field_type_to_string(inner)),
            FieldType::Identifier(name) => name.clone(),
            FieldType::Object(_) => "Object".to_string(),
        }
    }

    /// Get hover info for a field within an object access context (::{ })
    fn get_field_context_hover(
        &self,
        uri: &Url,
        position: Position,
        field_name: &str,
    ) -> Option<String> {
        let doc = self.documents.get(uri)?;
        let lines: Vec<&str> = doc.lines().collect();
        let line = lines.get(position.line as usize)?;
        let char_idx = position.character as usize;

        if char_idx > line.len() {
            return None;
        }

        let before_cursor = &line[..char_idx];

        // Check if we're inside ::{ } by finding last "::{" before cursor
        if let Some(brace_pos) = before_cursor.rfind("::{") {
            // Check no unmatched "}" between "::{" and cursor
            let between = &before_cursor[brace_pos + 3..];
            if between.contains('}') {
                return None; // We've exited the object access
            }

            // We're inside an object access - find the context type
            // Search for N<Type>, E<Type>, or V<Type> pattern before the "::{"
            let search_area = &before_cursor[..brace_pos];

            // Use a simple reverse search for the pattern
            if let Some((kind, type_name)) = self.find_context_type(search_area) {
                // Look up the field in the schema
                if let Some((field_type, full_type_path)) =
                    self.lookup_field_in_schema(uri, &kind, &type_name, field_name)
                {
                    return Some(format!("**{}**: {} (from {})", field_name, field_type, full_type_path));
                }
            }
        }

        None
    }

    /// Find the context type (N<Type>, E<Type>, or V<Type>) in a string
    fn find_context_type(&self, text: &str) -> Option<(String, String)> {
        // Search backwards for the most recent N<...>, E<...>, or V<...> pattern
        let mut best_match: Option<(usize, String, String)> = None;

        for pattern_char in ['N', 'E', 'V'] {
            let pattern = format!("{}<", pattern_char);
            if let Some(start) = text.rfind(&pattern) {
                let after_bracket = &text[start + 2..];
                if let Some(end) = after_bracket.find('>') {
                    let type_name = &after_bracket[..end];
                    // Only accept valid identifiers
                    if !type_name.is_empty() && type_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        if best_match.is_none() || start > best_match.as_ref().unwrap().0 {
                            best_match = Some((start, pattern_char.to_string(), type_name.to_string()));
                        }
                    }
                }
            }
        }

        best_match.map(|(_, kind, name)| (kind, name))
    }

    /// Look up a field type from a schema type
    fn lookup_field_in_schema(
        &self,
        uri: &Url,
        type_kind: &str, // "N", "E", or "V"
        type_name: &str,
        field_name: &str,
    ) -> Option<(String, String)> // Returns (field_type, full_type_path)
    {
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))?;

        let source = self.parsed_cache.get(&dir_key)?;

        // Check for built-in fields first
        match type_kind {
            "N" => {
                // Built-in node fields
                match field_name {
                    "id" => return Some(("Uuid".to_string(), format!("N::{}", type_name))),
                    "label" => return Some(("String".to_string(), format!("N::{}", type_name))),
                    _ => {}
                }
            }
            "E" => {
                // Built-in edge fields
                match field_name {
                    "id" => return Some(("Uuid".to_string(), format!("E::{}", type_name))),
                    "label" => return Some(("String".to_string(), format!("E::{}", type_name))),
                    "from_node" => return Some(("Uuid".to_string(), format!("E::{}", type_name))),
                    "to_node" => return Some(("Uuid".to_string(), format!("E::{}", type_name))),
                    _ => {}
                }
            }
            "V" => {
                // Built-in vector fields
                match field_name {
                    "id" => return Some(("Uuid".to_string(), format!("V::{}", type_name))),
                    "label" => return Some(("String".to_string(), format!("V::{}", type_name))),
                    "data" => return Some(("[F64]".to_string(), format!("V::{}", type_name))),
                    "score" => return Some(("F64".to_string(), format!("V::{}", type_name))),
                    _ => {}
                }
            }
            _ => {}
        }

        // Search in schema definitions
        for (_version, schema) in &source.schema {
            match type_kind {
                "N" => {
                    for node in &schema.node_schemas {
                        if node.name.1 == type_name {
                            for field in &node.fields {
                                if field.name == field_name {
                                    let type_str = Self::field_type_to_string(&field.field_type);
                                    return Some((type_str, format!("N::{}", type_name)));
                                }
                            }
                        }
                    }
                }
                "E" => {
                    for edge in &schema.edge_schemas {
                        if edge.name.1 == type_name {
                            if let Some(props) = &edge.properties {
                                for field in props {
                                    if field.name == field_name {
                                        let type_str = Self::field_type_to_string(&field.field_type);
                                        return Some((type_str, format!("E::{}", type_name)));
                                    }
                                }
                            }
                        }
                    }
                }
                "V" => {
                    for vector in &schema.vector_schemas {
                        if vector.name == type_name {
                            for field in &vector.fields {
                                if field.name == field_name {
                                    let type_str = Self::field_type_to_string(&field.field_type);
                                    return Some((type_str, format!("V::{}", type_name)));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        None
    }

    /// Get variable type from assignment context
    fn get_variable_type(&self, uri: &Url, position: Position, word: &str) -> Option<String> {
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))?;

        let source = self.parsed_cache.get(&dir_key)?;

        // Find query containing the position
        let line = position.line as usize + 1; // Parser uses 1-based line numbers

        for query in &source.queries {
            // Check if position is within this query
            let query_start = query.loc.start.line;
            let query_end = query.loc.end.line;

            if line >= query_start && line <= query_end {
                // Search statements for assignment
                return self.find_variable_in_statements(&query.statements, word);
            }
        }

        None
    }

    /// Get query parameter type for hover
    fn get_parameter_type(&self, uri: &Url, position: Position, word: &str) -> Option<String> {
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))?;

        let source = self.parsed_cache.get(&dir_key)?;
        let line = position.line as usize + 1; // Parser uses 1-based line numbers

        for query in &source.queries {
            // Check if position is within this query's range
            if line >= query.loc.start.line && line <= query.loc.end.line {
                // Check parameters
                for param in &query.parameters {
                    if param.name.1 == word {
                        let type_str = Self::field_type_to_string(&param.param_type.1);
                        let optional = if param.is_optional { "?" } else { "" };
                        return Some(format!(
                            "**{}**: {}{} (parameter)",
                            word, type_str, optional
                        ));
                    }
                }
            }
        }

        None
    }

    /// Search statements for a variable assignment and infer its type
    fn find_variable_in_statements(
        &self,
        statements: &[helix_db::helixc::parser::types::Statement],
        word: &str,
    ) -> Option<String> {
        for stmt in statements {
            match &stmt.statement {
                StatementType::Assignment(assignment) => {
                    if assignment.variable == word {
                        return self.infer_expression_type(&assignment.value.expr);
                    }
                }
                StatementType::ForLoop(for_loop) => {
                    // Check nested statements in for loop
                    if let Some(result) =
                        self.find_variable_in_statements(&for_loop.statements, word)
                    {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Infer the type from an expression
    fn infer_expression_type(&self, expr: &ExpressionType) -> Option<String> {
        match expr {
            ExpressionType::AddNode(add_node) => {
                if let Some(ref node_type) = add_node.node_type {
                    Some(format!("**{}**: Node<{}>", "variable", node_type))
                } else {
                    Some("**variable**: Node".to_string())
                }
            }
            ExpressionType::AddEdge(add_edge) => {
                if let Some(ref edge_type) = add_edge.edge_type {
                    Some(format!("**{}**: Edge<{}>", "variable", edge_type))
                } else {
                    Some("**variable**: Edge".to_string())
                }
            }
            ExpressionType::AddVector(add_vector) => {
                if let Some(ref vector_type) = add_vector.vector_type {
                    Some(format!("**{}**: Vector<{}>", "variable", vector_type))
                } else {
                    Some("**variable**: Vector".to_string())
                }
            }
            ExpressionType::Traversal(traversal) => {
                // Check steps for AddEdge (AddN doesn't appear in steps, it's its own expression)
                for step in &traversal.steps {
                    if let StepType::AddEdge(add_edge) = &step.step {
                        if let Some(ref t) = add_edge.edge_type {
                            return Some(format!("**variable**: Edge<{}>", t));
                        }
                        return Some("**variable**: Edge".to_string());
                    }
                }
                // Try to infer from the start node
                match &traversal.start {
                    helix_db::helixc::parser::types::StartNode::Node { node_type, .. } => {
                        Some(format!("**{}**: Node<{}>", "variable", node_type))
                    }
                    helix_db::helixc::parser::types::StartNode::Edge { edge_type, .. } => {
                        Some(format!("**{}**: Edge<{}>", "variable", edge_type))
                    }
                    helix_db::helixc::parser::types::StartNode::Vector { vector_type, .. } => {
                        Some(format!("**{}**: Vector<{}>", "variable", vector_type))
                    }
                    helix_db::helixc::parser::types::StartNode::SearchVector(sv) => {
                        if let Some(ref vt) = sv.vector_type {
                            Some(format!("**{}**: Vector<{}>", "variable", vt))
                        } else {
                            Some("**variable**: Vector".to_string())
                        }
                    }
                    _ => Some("**variable**: Traversal".to_string()),
                }
            }
            ExpressionType::SearchVector(sv) => {
                if let Some(ref vector_type) = sv.vector_type {
                    Some(format!("**{}**: Vector<{}>", "variable", vector_type))
                } else {
                    Some("**variable**: Vector".to_string())
                }
            }
            ExpressionType::StringLiteral(_) => Some("**variable**: String".to_string()),
            ExpressionType::IntegerLiteral(_) => Some("**variable**: Integer".to_string()),
            ExpressionType::FloatLiteral(_) => Some("**variable**: Float".to_string()),
            ExpressionType::BooleanLiteral(_) => Some("**variable**: Boolean".to_string()),
            ExpressionType::ArrayLiteral(_) => Some("**variable**: Array".to_string()),
            _ => None,
        }
    }

    /// Get documentation for a keyword/function
    fn get_keyword_docs(&self, word: &str) -> Option<String> {
        let docs: HashMap<&str, &str> = HashMap::from([
            // Query keywords
            (
                "QUERY",
                "Defines a new query function.\n\nSyntax: `QUERY name(param: Type) => { ... }`",
            ),
            ("RETURN", "Returns values from a query.\n\nSyntax: `RETURN expression`"),
            ("WHERE", "Filters results based on a condition.\n\nSyntax: `::WHERE(condition)`"),
            ("FOR", "Iterates over a collection.\n\nSyntax: `FOR item IN collection`"),
            ("IN", "Used with FOR loops or set membership."),
            ("EXISTS", "Checks if a value exists."),
            ("AND", "Logical AND operator."),
            ("OR", "Logical OR operator."),
            ("NONE", "Represents no value or empty result."),
            ("DROP", "Removes/deletes data."),
            ("MIGRATION", "Defines a schema migration."),
            ("FIRST", "Returns the first element of a collection."),
            ("AS", "Alias or type casting operator."),
            ("UNIQUE", "Ensures uniqueness constraint."),
            // Schema types
            (
                "N",
                "Node type definition.\n\nSyntax: `N::TypeName { field: Type }`",
            ),
            (
                "E",
                "Edge type definition.\n\nSyntax: `E::TypeName { From: NodeType, To: NodeType }`",
            ),
            ("V", "Vector type definition.\n\nSyntax: `V::TypeName { ... }`"),
            // Traversal operators
            (
                "Out",
                "Traverse outgoing edges.\n\nSyntax: `node::Out<EdgeType>`",
            ),
            (
                "In",
                "Traverse incoming edges.\n\nSyntax: `node::In<EdgeType>`",
            ),
            (
                "OutE",
                "Get outgoing edge objects.\n\nSyntax: `node::OutE<EdgeType>`",
            ),
            (
                "InE",
                "Get incoming edge objects.\n\nSyntax: `node::InE<EdgeType>`",
            ),
            ("FromN", "Get the source node of an edge."),
            ("ToN", "Get the target node of an edge."),
            ("FromV", "Get the source vertex of a vector edge."),
            ("ToV", "Get the target vertex of a vector edge."),
            ("ShortestPath", "Find shortest path between nodes."),
            ("ShortestPathBFS", "Find shortest path using BFS algorithm."),
            (
                "ShortestPathDijkstras",
                "Find shortest path using Dijkstra's algorithm.",
            ),
            ("ShortestPathAStar", "Find shortest path using A* algorithm."),
            (
                "COUNT",
                "Count the number of elements.\n\nSyntax: `collection::COUNT`",
            ),
            (
                "RANGE",
                "Select a range of elements.\n\nSyntax: `::RANGE(start, end)`",
            ),
            (
                "UPDATE",
                "Update node/edge properties.\n\nSyntax: `::UPDATE { field: value }`",
            ),
            ("ID", "Get the unique identifier of a node/edge."),
            ("PREFILTER", "Pre-filter results before main query execution."),
            ("AGGREGATE_BY", "Aggregate results by a field."),
            ("GROUP_BY", "Group results by a field."),
            ("ORDER", "Order results.\n\nSyntax: `::ORDER(field, Asc|Desc)`"),
            // Comparison operators
            ("GT", "Greater than comparison."),
            ("GTE", "Greater than or equal comparison."),
            ("LT", "Less than comparison."),
            ("LTE", "Less than or equal comparison."),
            ("EQ", "Equality comparison."),
            ("NEQ", "Not equal comparison."),
            ("CONTAINS", "Check if collection contains value."),
            ("IS_IN", "Check if value is in collection."),
            // Creation operators
            (
                "AddN",
                "Add a new node.\n\nSyntax: `AddN<NodeType> { field: value }`",
            ),
            (
                "AddE",
                "Add a new edge.\n\nSyntax: `AddE<EdgeType> { From: node1, To: node2 }`",
            ),
            (
                "AddV",
                "Add a new vector.\n\nSyntax: `AddV<VectorType> { ... }`",
            ),
            ("BatchAddV", "Add multiple vectors in batch."),
            (
                "SearchV",
                "Search vectors by similarity.\n\nSyntax: `SearchV<VectorType>(query, k)`",
            ),
            ("SearchBM25", "Search using BM25 algorithm."),
            ("Embed", "Generate embeddings for text."),
            ("UpsertN", "Insert or update a node."),
            ("UpsertE", "Insert or update an edge."),
            ("UpsertV", "Insert or update a vector."),
            ("RerankRRF", "Rerank results using Reciprocal Rank Fusion."),
            (
                "RerankMMR",
                "Rerank results using Maximal Marginal Relevance.",
            ),
            // Properties
            (
                "From",
                "Source node of an edge.\n\nSyntax: `From: NodeType`",
            ),
            ("To", "Target node of an edge.\n\nSyntax: `To: NodeType`"),
            ("Properties", "Define edge properties."),
            ("INDEX", "Create an index on a field."),
            ("DEFAULT", "Set a default value for a field."),
            ("NOW", "Current timestamp function."),
            // Order direction
            ("Asc", "Ascending order."),
            ("Desc", "Descending order."),
            // Math functions
            ("ADD", "Addition operation."),
            ("SUB", "Subtraction operation."),
            ("MUL", "Multiplication operation."),
            ("DIV", "Division operation."),
            ("POW", "Power/exponentiation operation."),
            ("MOD", "Modulo operation."),
            ("ABS", "Absolute value."),
            ("SQRT", "Square root."),
            ("LN", "Natural logarithm."),
            ("LOG10", "Base-10 logarithm."),
            ("LOG", "Logarithm."),
            ("EXP", "Exponential function."),
            ("CEIL", "Ceiling function."),
            ("FLOOR", "Floor function."),
            ("ROUND", "Round to nearest integer."),
            ("SIN", "Sine function."),
            ("COS", "Cosine function."),
            ("TAN", "Tangent function."),
            ("ASIN", "Arc sine function."),
            ("ACOS", "Arc cosine function."),
            ("ATAN", "Arc tangent function."),
            ("ATAN2", "Two-argument arc tangent."),
            ("PI", "Mathematical constant pi."),
            ("MIN", "Minimum value."),
            ("MAX", "Maximum value."),
            ("SUM", "Sum of values."),
            ("AVG", "Average of values."),
            // Types
            ("String", "String/text type."),
            ("Boolean", "Boolean (true/false) type."),
            ("F32", "32-bit floating point number."),
            ("F64", "64-bit floating point number."),
            ("I8", "8-bit signed integer."),
            ("I16", "16-bit signed integer."),
            ("I32", "32-bit signed integer."),
            ("I64", "64-bit signed integer."),
            ("U8", "8-bit unsigned integer."),
            ("U16", "16-bit unsigned integer."),
            ("U32", "32-bit unsigned integer."),
            ("U64", "64-bit unsigned integer."),
            ("U128", "128-bit unsigned integer."),
            ("Date", "Date/timestamp type."),
            ("Uuid", "UUID type."),
        ]);

        docs.get(word).map(|s| s.to_string())
    }

    /// Find definition location for a type reference
    fn find_definition(&self, uri: &Url, word: &str) -> Option<Location> {
        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))?;

        let source = self.parsed_cache.get(&dir_key)?;

        // Check all schema versions
        for (_version, schema) in &source.schema {
            // Check node definitions
            for node in &schema.node_schemas {
                if node.name.1 == word {
                    let file_name = node.name.0.filepath.as_deref().unwrap_or("");
                    let file_path = Path::new(&dir_key).join(file_name);
                    if let Ok(file_uri) = Url::from_file_path(&file_path) {
                        return Some(Location {
                            uri: file_uri,
                            range: Range {
                                start: Position {
                                    line: node.name.0.start.line.saturating_sub(1) as u32,
                                    character: node.name.0.start.column.saturating_sub(1) as u32,
                                },
                                end: Position {
                                    line: node.name.0.end.line.saturating_sub(1) as u32,
                                    character: node.name.0.end.column.saturating_sub(1) as u32,
                                },
                            },
                        });
                    }
                }
            }

            // Check edge definitions
            for edge in &schema.edge_schemas {
                if edge.name.1 == word {
                    let file_name = edge.name.0.filepath.as_deref().unwrap_or("");
                    let file_path = Path::new(&dir_key).join(file_name);
                    if let Ok(file_uri) = Url::from_file_path(&file_path) {
                        return Some(Location {
                            uri: file_uri,
                            range: Range {
                                start: Position {
                                    line: edge.name.0.start.line.saturating_sub(1) as u32,
                                    character: edge.name.0.start.column.saturating_sub(1) as u32,
                                },
                                end: Position {
                                    line: edge.name.0.end.line.saturating_sub(1) as u32,
                                    character: edge.name.0.end.column.saturating_sub(1) as u32,
                                },
                            },
                        });
                    }
                }
            }

            // Check vector definitions
            for vector in &schema.vector_schemas {
                if vector.name == word {
                    let file_name = vector.loc.filepath.as_deref().unwrap_or("");
                    let file_path = Path::new(&dir_key).join(file_name);
                    if let Ok(file_uri) = Url::from_file_path(&file_path) {
                        return Some(Location {
                            uri: file_uri,
                            range: Range {
                                start: Position {
                                    line: vector.loc.start.line.saturating_sub(1) as u32,
                                    character: vector.loc.start.column.saturating_sub(1) as u32,
                                },
                                end: Position {
                                    line: vector.loc.end.line.saturating_sub(1) as u32,
                                    character: vector.loc.end.column.saturating_sub(1) as u32,
                                },
                            },
                        });
                    }
                }
            }
        }

        None
    }

    /// Get completion items based on context
    fn get_completions(&self, uri: &Url, position: Position) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        let doc = match self.documents.get(uri) {
            Some(d) => d.clone(),
            None => return items,
        };

        let lines: Vec<&str> = doc.lines().collect();
        let line = match lines.get(position.line as usize) {
            Some(l) => *l,
            None => return items,
        };

        let char_idx = position.character as usize;
        let prefix = if char_idx <= line.len() {
            &line[..char_idx]
        } else {
            line
        };

        // Check context
        let after_n_bracket =
            prefix.ends_with("N<") || prefix.contains("N<") && !prefix.contains('>');
        let after_e_bracket =
            prefix.ends_with("E<") || prefix.contains("E<") && !prefix.contains('>');
        let after_v_bracket =
            prefix.ends_with("V<") || prefix.contains("V<") && !prefix.contains('>');
        let after_double_colon = prefix.ends_with("::");

        let dir_key = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()));

        // Type completions after N<, E<, V<
        if let Some(ref key) = dir_key {
            if let Some(source) = self.parsed_cache.get(key) {
                if after_n_bracket {
                    for (_version, schema) in &source.schema {
                        for node in &schema.node_schemas {
                            items.push(CompletionItem {
                                label: node.name.1.clone(),
                                kind: Some(CompletionItemKind::CLASS),
                                detail: Some("Node type".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                    return items;
                }

                if after_e_bracket {
                    for (_version, schema) in &source.schema {
                        for edge in &schema.edge_schemas {
                            items.push(CompletionItem {
                                label: edge.name.1.clone(),
                                kind: Some(CompletionItemKind::CLASS),
                                detail: Some("Edge type".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                    return items;
                }

                if after_v_bracket {
                    for (_version, schema) in &source.schema {
                        for vector in &schema.vector_schemas {
                            items.push(CompletionItem {
                                label: vector.name.clone(),
                                kind: Some(CompletionItemKind::CLASS),
                                detail: Some("Vector type".to_string()),
                                ..Default::default()
                            });
                        }
                    }
                    return items;
                }
            }
        }

        // Traversal completions after ::
        if after_double_colon {
            let traversals = [
                ("Out", "Traverse outgoing edges"),
                ("In", "Traverse incoming edges"),
                ("OutE", "Get outgoing edge objects"),
                ("InE", "Get incoming edge objects"),
                ("FromN", "Get source node"),
                ("ToN", "Get target node"),
                ("FromV", "Get source vertex"),
                ("ToV", "Get target vertex"),
                ("From", "Edge source (in AddE chain)"),
                ("To", "Edge target (in AddE chain)"),
                ("WHERE", "Filter by condition"),
                ("COUNT", "Count elements"),
                ("RANGE", "Select range"),
                ("UPDATE", "Update properties"),
                ("ID", "Get identifier"),
                ("GT", "Greater than"),
                ("GTE", "Greater than or equal"),
                ("LT", "Less than"),
                ("LTE", "Less than or equal"),
                ("EQ", "Equal"),
                ("NEQ", "Not equal"),
                ("CONTAINS", "Contains value"),
                ("IS_IN", "Value in set"),
                ("PREFILTER", "Pre-filter results"),
                ("AGGREGATE_BY", "Aggregate by field"),
                ("GROUP_BY", "Group by field"),
                ("ORDER", "Order results"),
                ("ShortestPath", "Find shortest path"),
                ("ShortestPathBFS", "Shortest path (BFS)"),
                ("ShortestPathDijkstras", "Shortest path (Dijkstra)"),
                ("ShortestPathAStar", "Shortest path (A*)"),
            ];

            for (name, detail) in traversals {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some(detail.to_string()),
                    ..Default::default()
                });
            }
            return items;
        }

        // Default keyword completions
        let keywords = [
            ("QUERY", "Define a query", CompletionItemKind::KEYWORD),
            ("RETURN", "Return values", CompletionItemKind::KEYWORD),
            ("WHERE", "Filter condition", CompletionItemKind::KEYWORD),
            ("FOR", "Loop iteration", CompletionItemKind::KEYWORD),
            ("IN", "In operator", CompletionItemKind::KEYWORD),
            ("AND", "Logical AND", CompletionItemKind::KEYWORD),
            ("OR", "Logical OR", CompletionItemKind::KEYWORD),
            ("EXISTS", "Check existence", CompletionItemKind::KEYWORD),
            ("NONE", "No value", CompletionItemKind::KEYWORD),
            ("DROP", "Delete data", CompletionItemKind::KEYWORD),
            ("MIGRATION", "Schema migration", CompletionItemKind::KEYWORD),
            ("FIRST", "First element", CompletionItemKind::KEYWORD),
            ("AS", "Alias/cast", CompletionItemKind::KEYWORD),
            ("UNIQUE", "Uniqueness constraint", CompletionItemKind::KEYWORD),
            ("AddN", "Add node", CompletionItemKind::FUNCTION),
            ("AddE", "Add edge", CompletionItemKind::FUNCTION),
            ("AddV", "Add vector", CompletionItemKind::FUNCTION),
            ("BatchAddV", "Batch add vectors", CompletionItemKind::FUNCTION),
            ("SearchV", "Search vectors", CompletionItemKind::FUNCTION),
            ("SearchBM25", "BM25 search", CompletionItemKind::FUNCTION),
            ("Embed", "Generate embeddings", CompletionItemKind::FUNCTION),
            ("UpsertN", "Upsert node", CompletionItemKind::FUNCTION),
            ("UpsertE", "Upsert edge", CompletionItemKind::FUNCTION),
            ("UpsertV", "Upsert vector", CompletionItemKind::FUNCTION),
            ("RerankRRF", "Rerank (RRF)", CompletionItemKind::FUNCTION),
            ("RerankMMR", "Rerank (MMR)", CompletionItemKind::FUNCTION),
        ];

        for (name, detail, kind) in keywords {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(kind),
                detail: Some(detail.to_string()),
                ..Default::default()
            });
        }

        items
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "HelixQL Language Server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![":".to_string(), "<".to_string()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "HelixQL Language Server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text);
        self.analyze_workspace(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.insert(uri.clone(), change.text);
            self.analyze_workspace(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.analyze_workspace(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(word) = self.get_word_at_position(uri, position) {
            // First check for keyword documentation
            if let Some(docs) = self.get_keyword_docs(&word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("**{}**\n\n{}", word, docs),
                    }),
                    range: None,
                }));
            }

            // Check if it's a schema type (Node, Edge, or Vector)
            if let Some(type_info) = self.get_type_hover_info(uri, &word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: type_info,
                    }),
                    range: None,
                }));
            }

            // Check if it's a variable - infer its type
            if let Some(var_type) = self.get_variable_type(uri, position, &word) {
                // Replace "variable" placeholder with the actual variable name
                let var_info = var_type.replace("**variable**", &format!("**{}**", word));
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: var_info,
                    }),
                    range: None,
                }));
            }

            // Check if it's a query parameter
            if let Some(param_info) = self.get_parameter_type(uri, position, &word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: param_info,
                    }),
                    range: None,
                }));
            }

            // Check if it's a field in an object access context (::{ })
            if let Some(field_info) = self.get_field_context_hover(uri, position, &word) {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: field_info,
                    }),
                    range: None,
                }));
            }
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(word) = self.get_word_at_position(uri, position) {
            if let Some(location) = self.find_definition(uri, &word) {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let items = self.get_completions(uri, position);

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
