use std::collections::HashMap;

use dashmap::DashMap;
use pest::Parser;
use pest_derive::Parser;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Parser)]
#[grammar = "grammar.pest"]
pub struct HelixQueryParser;

#[derive(Debug)]
enum CompletionContext {
    Root,
    QueryDefinition,
    Type,
    ParameterType,
    GetClause,
    EntityType,
    Relationship,
    Unknown,
}

#[derive(Debug, Clone)]
struct VariableInfo {
    name: String,
    var_type: String,
    definition_range: Range,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    document_symbols: DashMap<String, DashMap<String, Position>>,
}

impl Backend {
    fn get_hover_text(&self, word: &str) -> Option<String> {
        let keyword_docs: HashMap<&str, &str> = [
            ("QUERY", "**QUERY** - Defines a new query operation.\n\nExample:\n```\nQUERY findPerson(name: String) => ...\n```"),
            ("GET", "**GET** - Retrieves data from the graph.\n\nExample:\n```\nGET person <- V::Person\n```"),
            ("RETURN", "**RETURN** - Specifies which variables to return from the query.\n\nExample:\n```\nRETURN person\n```"),
            ("V", "**V** (Vertex) - Specifies a vertex traversal in the graph.\n\nExample:\n```\nV::Person\n```"),
            ("E", "**E** (Edge) - Specifies an edge traversal in the graph.\n\nExample:\n```\nE::WORKS_AT\n```"),
            ("String", "**String** - A text data type.\n\nExample:\n```\nname: String\n```"),
            ("Integer", "**Integer** - A numeric data type that represents integers.\n\nExample:\n```\nage: Integer\n```"),
            ("Float", "**Float** - A numeric data type that represents floating-point numbers.\n\nExample:\n```\nweight: Float\n```"),
            ("Boolean", "**Boolean** - A logical data type that can be either true or false.\n\nExample:\n```\nactive: Boolean\n```"),
            ("Out", "**Out** - Specifies an outgoing edge traversal.\n\nExample:\n```V::Out\n```"),
            ("In", "**In** - Specifies an incoming edge traversal.\n\nExample:\n```V::In\n```"),
            ("OutE", "**OutE** - Specifies an outgoing edge traversal.\n\nExample:\n```V::OutE\n```"),
            ("InE", "**InE** - Specifies an incoming edge traversal.\n\nExample:\n```V::InE\n```"),
        ].iter().cloned().collect();

        keyword_docs.get(word).map(|&s| s.to_string())
    }

    fn get_line_until_cursor(&self, content: &str, position: &Position) -> String {
        content
            .lines()
            .nth(position.line as usize)
            .map(|line| line[..position.character as usize].to_string())
            .unwrap_or_default()
    }

    fn get_completion_context(&self, content: &str, position: &Position) -> CompletionContext {
        let line = self.get_line_until_cursor(content, position);
        let trimmed = line.trim();
        let current_line = content.lines().nth(position.line as usize).unwrap_or("");

        // Ignore comments
        if trimmed.starts_with("/*") {
            return CompletionContext::Unknown;
        }

        if trimmed.is_empty() || trimmed.ends_with(' ') {
            if content.contains("=>") && !content.contains("RETURN") {
                return CompletionContext::GetClause;
            }
            return CompletionContext::Root;
        }

        // Query definition
        if trimmed.starts_with("QUERY ") {
            if trimmed.contains('(') {
                if trimmed.contains(':') {
                    return CompletionContext::Type;
                }
                return CompletionContext::ParameterType;
            }
            return CompletionContext::QueryDefinition;
        }

        // After => symbol
        if current_line.contains("=>") || content.contains("=>") {
            if trimmed.ends_with("V::") || trimmed.ends_with("E::") {
                return CompletionContext::EntityType;
            }

            if trimmed.contains("::") && (trimmed.ends_with("::") || trimmed.ends_with("(")) {
                return CompletionContext::Relationship;
            }

            return CompletionContext::GetClause;
        }

        CompletionContext::Unknown
    }

    fn get_completions_for_context(&self, context: CompletionContext) -> Vec<CompletionItem> {
        match context {
            CompletionContext::Root => vec![CompletionItem {
                label: "QUERY".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Define a new query".to_string()),
                insert_text: Some("QUERY $1($2) =>\n    GET $3 <- ".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            }],
            CompletionContext::Type | CompletionContext::ParameterType => vec![
                CompletionItem {
                    label: "String".to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some("String type".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "Integer".to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some("Integer type".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "Float".to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some("Float type".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "Boolean".to_string(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some("Boolean type".to_string()),
                    ..Default::default()
                },
            ],
            CompletionContext::GetClause => vec![
                CompletionItem {
                    label: "GET".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("Retrieve data".to_string()),
                    insert_text: Some("GET $1 <- ".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
                CompletionItem {
                    label: "RETURN".to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("Specify return values".to_string()),
                    insert_text: Some("RETURN $1".to_string()),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                },
            ],
            CompletionContext::EntityType => vec![
                CompletionItem {
                    label: "Person".to_string(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("Person entity".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "Company".to_string(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("Company entity".to_string()),
                    ..Default::default()
                },
            ],
            CompletionContext::Relationship => vec![
                CompletionItem {
                    label: "In".to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some("Incoming edge traversal".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "Out".to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some("Outgoing edge traversal".to_string()),
                    ..Default::default()
                },
                CompletionItem {
                    label: "WorksAt".to_string(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    detail: Some("Works at relationship".to_string()),
                    ..Default::default()
                },
            ],
            _ => vec![],
        }
    }

    fn parse_variables(&self, content: &str) -> HashMap<String, VariableInfo> {
        let mut variables = HashMap::new();
        let lines: Vec<&str> = content.lines().collect();

        for (line_idx, line) in lines.iter().enumerate() {
            let line = line.trim();

            // Parse query parameters
            if line.starts_with("QUERY") {
                if let Some(params_start) = line.find('(') {
                    if let Some(params_end) = line.find(')') {
                        let params = &line[params_start + 1..params_end];
                        if !params.is_empty() {
                            let parts: Vec<&str> = params.split(':').map(|s| s.trim()).collect();
                            if parts.len() == 2 {
                                let var_name = parts[0].to_string();
                                let var_type = parts[1].to_string();
                                variables.insert(
                                    var_name.clone(),
                                    VariableInfo {
                                        name: var_name,
                                        var_type,
                                        definition_range: Range {
                                            start: Position::new(
                                                line_idx as u32,
                                                line.find(parts[0]).unwrap_or(0) as u32,
                                            ),
                                            end: Position::new(
                                                line_idx as u32,
                                                (line.find(parts[1]).unwrap_or(0) + parts[1].len())
                                                    as u32,
                                            ),
                                        },
                                    },
                                );
                            }
                        }
                    }
                }
                continue;
            }

            // Parse RETURN statement to include multiple variables
            if line.starts_with("RETURN") {
                let return_vars = line.split("RETURN").nth(1).unwrap_or("").split(',');
                for var_name in return_vars {
                    let var_name = var_name.trim();
                    if !var_name.is_empty() && !variables.contains_key(var_name) {
                        variables.insert(
                            var_name.to_string(),
                            VariableInfo {
                                name: var_name.to_string(),
                                var_type: "Unknown".to_string(),
                                definition_range: Range {
                                    start: Position::new(
                                        line_idx as u32,
                                        line.find(var_name).unwrap_or(0) as u32,
                                    ),
                                    end: Position::new(
                                        line_idx as u32,
                                        (line.find(var_name).unwrap_or(0) + var_name.len()) as u32,
                                    ),
                                },
                            },
                        );
                    }
                }
                continue;
            }

            // Parse variable assignments
            if line.contains("<-") {
                let parts: Vec<&str> = line.split("<-").collect();
                if let Some(var_part) = parts.first() {
                    let var_name = if var_part.trim().starts_with("GET") {
                        var_part.trim().split("GET").nth(1).unwrap_or("").trim()
                    } else {
                        var_part.trim()
                    };

                    if !var_name.is_empty() {
                        if let Some(pattern) = parts.get(1) {
                            let pattern = pattern.trim();

                            let var_type =
                                if pattern.starts_with("V::") || pattern.starts_with("E::") {
                                    pattern.split("::").nth(1).unwrap_or("Unknown").to_string()
                                } else {
                                    pattern.split("::").last().unwrap_or("Unknown").to_string()
                                };

                            variables.insert(
                                var_name.to_string(),
                                VariableInfo {
                                    name: var_name.to_string(),
                                    var_type,
                                    definition_range: Range {
                                        start: Position::new(
                                            line_idx as u32,
                                            line.find(var_name).unwrap_or(0) as u32,
                                        ),
                                        end: Position::new(
                                            line_idx as u32,
                                            (line.find(var_name).unwrap_or(0) + var_name.len())
                                                as u32,
                                        ),
                                    },
                                },
                            );
                        }
                    }
                }
            }
        }

        variables
    }
    fn get_word_at_position(&self, content: &str, position: &Position) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        if let Some(line) = lines.get(position.line as usize) {
            let char_pos = position.character as usize;
            if char_pos >= line.len() {
                return None;
            }

            let before = &line[..=char_pos];
            let after = &line[char_pos..];

            let word_start = before
                .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| i + 1)
                .unwrap_or(0);
            let word_end = char_pos
                + after
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(after.len());

            Some(line[word_start..word_end].to_string())
        } else {
            None
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(true),
                    trigger_characters: Some(vec![
                        " ".to_string(),
                        ":".to_string(),
                        "(".to_string(),
                        ",".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: Some(CompletionOptionsCompletionItem {
                        label_details_support: Some(true),
                    }),
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),

                ..ServerCapabilities::default()
            },
            server_info: None,
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "Helix Query Language Server initialised!",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Ok(content) = std::fs::read_to_string(uri.path()) {
            let context = self.get_completion_context(&content, &position);
            let items = self.get_completions_for_context(context);
            return Ok(Some(CompletionResponse::Array(items)));
        }

        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        if let Ok(content) = std::fs::read_to_string(uri.path()) {
            if let Some(word) = self.get_word_at_position(&content, &position) {
                if let Some(hover_text) = self.get_hover_text(&word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: hover_text,
                        }),
                        range: None,
                    }));
                }

                let variables = self.parse_variables(&content);
                if let Some(var_info) = variables.get(&word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!(
                                "**Variable**: {}\n**Type**: {}",
                                var_info.name, var_info.var_type
                            ),
                        }),
                        range: Some(var_info.definition_range),
                    }));
                }
            }
        }

        Ok(None)
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.validate_document(&params.text_document.uri, &params.content_changes[0].text)
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.validate_document(&params.text_document.uri, &params.text_document.text)
            .await;
    }
}

impl Backend {
    fn create_diagnostic_from_error(err: pest::error::Error<Rule>) -> Diagnostic {
        let error_location = match err.line_col {
            pest::error::LineColLocation::Pos((line, col)) => (line, col),
            pest::error::LineColLocation::Span((line, col), _) => (line, col),
        };

        let range = Range {
            start: Position::new((error_location.0 - 1) as u32, (error_location.1 - 1) as u32),
            end: Position::new((error_location.0 - 1) as u32, error_location.1 as u32),
        };

        Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            message: err.to_string(),
            source: Some("helix-query-lsp".to_string()),
            code: None,
            code_description: None,
            tags: None,
            related_information: None,
            data: None,
        }
    }

    async fn validate_document(&self, uri: &Url, content: &str) {
        let diagnostics = match HelixQueryParser::parse(Rule::source, content) {
            Ok(_) => Vec::new(),
            Err(err) => {
                let diagnostic = Self::create_diagnostic_from_error(err);
                vec![diagnostic]
            }
        };

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::build(|client| Backend {
        client,
        document_symbols: DashMap::new(),
    })
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}
