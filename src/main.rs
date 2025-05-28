use dashmap::DashMap;
use helixdb::helixc::{
    analyzer::analyzer::{analyze, Diagnostic as HelixDiagnostic, DiagnosticSeverity as HelixSeverity},
    parser::helix_parser::{Content, HxFile, HelixParser},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: Arc<DashMap<Url, String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct InlayHintParams {
    path: String,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // Remove diagnostic provider for now to avoid method not found errors
                // diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                //     DiagnosticOptions {
                //         inter_file_dependencies: true,
                //         workspace_diagnostics: true,
                //         ..Default::default()
                //     },
                // )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "HelixQL LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        
        // Log file opening for debugging
        self.client
            .log_message(MessageType::INFO, format!("Opening file: {}", uri.path()))
            .await;
        
        // Store document
        self.documents.insert(uri.clone(), text.clone());
        
        // Run diagnostics
        self.run_diagnostics(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(changes) = params.content_changes.first() {
            self.documents.insert(uri.clone(), changes.text.clone());
            self.run_diagnostics(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Re-run diagnostics on save
        self.run_diagnostics(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
        // Clear diagnostics for closed file
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        
        // Get document content
        let content = match self.documents.get(&uri) {
            Some(doc) => doc.clone(),
            None => return Ok(None),
        };
        
        // Simple hover for now - can be enhanced with type information from analyzer
        let lines: Vec<&str> = content.lines().collect();
        if let Some(line) = lines.get(position.line as usize) {
            let hover_text = self.get_hover_info(line, position.character as usize);
            if let Some(text) = hover_text {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: text,
                    }),
                    range: None,
                }));
            }
        }
        
        Ok(None)
    }
}

impl Backend {
    async fn run_diagnostics(&self, uri: &Url) {
        // Log diagnostic run for debugging
        self.client
            .log_message(MessageType::INFO, format!("Running diagnostics for: {}", uri.path()))
            .await;
            
        // Get the directory of the current file
        let current_dir = std::path::Path::new(uri.path())
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
            
        self.client
            .log_message(MessageType::INFO, format!("Analyzing files in directory: {}", current_dir))
            .await;
            
        // Collect all .hx and .hql files in the SAME DIRECTORY as the opened file
        let files: Vec<HxFile> = self.documents
            .iter()
            .filter_map(|entry| {
                let file_uri = entry.key();
                let file_content = entry.value();
                
                // Get the directory of this file
                let file_dir = std::path::Path::new(file_uri.path())
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                
                // Only include .hx and .hql files from the same directory
                if (file_uri.path().ends_with(".hx") || file_uri.path().ends_with(".hql")) 
                    && file_dir == current_dir {
                    Some(HxFile {
                        name: file_uri.path().to_string(),
                        content: file_content.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        if files.is_empty() {
            self.client
                .log_message(MessageType::INFO, format!("No .hx or .hql files found in directory: {}", current_dir))
                .await;
            return;
        }

        self.client
            .log_message(MessageType::INFO, format!("Analyzing {} files in directory: {}", files.len(), current_dir))
            .await;

        // Create content structure (like CLI)
        let content = Content {
            content: String::new(),
            source: Default::default(),
            files,
        };

        // Parse and analyze (like CLI)
        match HelixParser::parse_source(&content) {
            Ok(parsed) => {
                let (diagnostics, _) = analyze(&parsed);
                
                self.client
                    .log_message(MessageType::INFO, format!("Found {} diagnostics", diagnostics.len()))
                    .await;
                
                // Group diagnostics by file path
                let mut diags_by_file: std::collections::HashMap<String, Vec<Diagnostic>> = 
                    std::collections::HashMap::new();
                
                for diag in diagnostics {
                    // Get the file path from the diagnostic
                    let file_path = diag.filepath.as_ref()
                        .or_else(|| diag.location.filepath.as_ref())
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    let lsp_diag = self.convert_diagnostic(&diag);
                    diags_by_file.entry(file_path).or_default().push(lsp_diag);
                }
                
                // Clear diagnostics for all files in the same directory first, then publish new ones
                for entry in self.documents.iter() {
                    let file_uri = entry.key();
                    
                    // Get the directory of this file
                    let file_dir = std::path::Path::new(file_uri.path())
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string());
                    
                    // Only publish diagnostics for files in the same directory
                    if (file_uri.path().ends_with(".hx") || file_uri.path().ends_with(".hql")) 
                        && file_dir == current_dir {
                        let file_path = file_uri.path().to_string();
                        let diagnostics = diags_by_file.get(&file_path).cloned().unwrap_or_default();
                        
                        self.client
                            .publish_diagnostics(file_uri.clone(), diagnostics, None)
                            .await;
                    }
                }
            }
            Err(e) => {
                // Parser error - publish to files in the same directory
                let error_message = format!("Parse error: {}", e);
                
                self.client
                    .log_message(MessageType::ERROR, error_message.clone())
                    .await;
                
                let diagnostic = Diagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: error_message,
                    source: Some("helixql".to_string()),
                    ..Default::default()
                };
                
                // Publish parse error to files in the same directory
                for entry in self.documents.iter() {
                    let file_uri = entry.key();
                    
                    // Get the directory of this file
                    let file_dir = std::path::Path::new(file_uri.path())
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string());
                    
                    // Only publish errors to files in the same directory
                    if (file_uri.path().ends_with(".hx") || file_uri.path().ends_with(".hql")) 
                        && file_dir == current_dir {
                        self.client
                            .publish_diagnostics(file_uri.clone(), vec![diagnostic.clone()], None)
                            .await;
                    }
                }
            }
        }
    }

    fn convert_diagnostic(&self, diag: &HelixDiagnostic) -> Diagnostic {
        // Get line and column information from the location
        // Assuming Loc has start and end with line/column fields
        // Adjust these based on your actual Loc structure
        
        // LSP uses 0-based line and column indices
        // If your analyzer uses 1-based indices, subtract 1
        let start_line = diag.location.start.line.saturating_sub(1) as u32;
        let start_col = diag.location.start.column.saturating_sub(1) as u32;
        let end_line = diag.location.end.line.saturating_sub(1) as u32;
        let end_col = diag.location.end.column.saturating_sub(1) as u32;
        
        // Convert severity
        let severity = match diag.severity {
            HelixSeverity::Error => Some(DiagnosticSeverity::ERROR),
            HelixSeverity::Warning => Some(DiagnosticSeverity::WARNING),
            HelixSeverity::Info => Some(DiagnosticSeverity::INFORMATION),
            HelixSeverity::Hint => Some(DiagnosticSeverity::HINT),
            HelixSeverity::Empty => None,
        };
        
        // Build the diagnostic message
        let mut message = diag.message.clone();
        if let Some(hint) = &diag.hint {
            message.push_str("\n\n");
            message.push_str("Hint: ");
            message.push_str(hint);
        }
        
        Diagnostic {
            range: Range::new(
                Position::new(start_line, start_col),
                Position::new(end_line, end_col),
            ),
            severity,
            message,
            source: Some("helixql".to_string()),
            code: None,
            code_description: None,
            tags: None,
            related_information: None,
            data: None,
        }
    }
    
    fn get_hover_info(&self, line: &str, char_pos: usize) -> Option<String> {
        // Enhanced hover information
        let hover_map = vec![
            // Creation operations
            ("AddN", "**AddN\\<Type\\>** - Create a new node\n\n```helixql\nAddN<User>({name: \"Alice\"})\n```"),
            ("AddE", "**AddE\\<Type\\>** - Create a new edge\n\n```helixql\nAddE<Follows>::From(user1)::To(user2)\n```"),
            ("AddV", "**AddV\\<Type\\>** - Add a vector\n\n```helixql\nAddV<Document>(vector, {content: \"text\"})\n```"),
            
            // Traversal operations
            ("Out", "**Out\\<EdgeType\\>** - Traverse outgoing edges to connected nodes"),
            ("In", "**In\\<EdgeType\\>** - Traverse incoming edges to connected nodes"),
            ("OutE", "**OutE\\<EdgeType\\>** - Get outgoing edges"),
            ("InE", "**InE\\<EdgeType\\>** - Get incoming edges"),
            ("FromN", "**FromN** - Get the source node of an edge"),
            ("ToN", "**ToN** - Get the target node of an edge"),
            
            // Filtering and conditions
            ("WHERE", "**WHERE** - Filter elements based on conditions\n\n```helixql\n::WHERE(_::{age}::GT(18))\n```"),
            ("EXISTS", "**EXISTS** - Check if traversal has results\n\n```helixql\nEXISTS(_::Out<Follows>)\n```"),
            ("AND", "**AND** - Logical AND operation"),
            ("OR", "**OR** - Logical OR operation"),
            
            // Comparison operations
            ("GT", "**GT** - Greater than"),
            ("GTE", "**GTE** - Greater than or equal"),
            ("LT", "**LT** - Less than"),
            ("LTE", "**LTE** - Less than or equal"),
            ("EQ", "**EQ** - Equal to"),
            ("NEQ", "**NEQ** - Not equal to"),
            
            // Other operations
            ("COUNT", "**COUNT** - Count the number of elements"),
            ("UPDATE", "**UPDATE** - Update properties of elements"),
            ("DROP", "**DROP** - Delete elements from the graph"),
            ("RANGE", "**RANGE** - Get a range of elements"),
            ("SearchV", "**SearchV** - Search for vectors"),
            
            // Types
            ("String", "**String** - Text data type"),
            ("Boolean", "**Boolean** - True/false value"),
            ("I8", "**I8** - 8-bit signed integer (-128 to 127)"),
            ("I16", "**I16** - 16-bit signed integer"),
            ("I32", "**I32** - 32-bit signed integer"),
            ("I64", "**I64** - 64-bit signed integer"),
            ("U8", "**U8** - 8-bit unsigned integer (0 to 255)"),
            ("U16", "**U16** - 16-bit unsigned integer"),
            ("U32", "**U32** - 32-bit unsigned integer"),
            ("U64", "**U64** - 64-bit unsigned integer"),
            ("U128", "**U128** - 128-bit unsigned integer"),
            ("F32", "**F32** - 32-bit floating point"),
            ("F64", "**F64** - 64-bit floating point"),
            ("ID", "**ID** - UUID identifier"),
            ("Date", "**Date** - Date/timestamp value"),
            
            // Keywords
            ("QUERY", "**QUERY** - Define a query function"),
            ("RETURN", "**RETURN** - Specify query output"),
            ("FOR", "**FOR** - Loop over a collection"),
            ("IN", "**IN** - Part of FOR loop syntax"),
            ("INDEX", "**INDEX** - Mark a field as indexed"),
            ("DEFAULT", "**DEFAULT** - Set default value for a field"),
        ];
        
        // Find the word at the cursor position
        let start = line[..char_pos.min(line.len())]
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        
        let end = line[char_pos..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| char_pos + i)
            .unwrap_or(line.len());
        
        if start < end {
            let word = &line[start..end];
            
            for (keyword, info) in hover_map {
                if word == keyword {
                    return Some(info.to_string());
                }
            }
        }
        
        None
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Arc::new(DashMap::new()),
    });
    
    Server::new(stdin, stdout, socket).serve(service).await;
}