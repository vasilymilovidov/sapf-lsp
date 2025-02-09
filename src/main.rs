mod dict;
use dict::VALUES_JSON;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Deserialize, Serialize, Debug)]
pub struct CategoryData {
    pub description: String,
    pub items: HashMap<String, String>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, String>>>,
    categories: HashMap<String, CategoryData>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            categories: load_categories(),
        }
    }

    async fn get_document_content(&self, uri: &Url) -> Option<String> {
        self.documents.read().await.get(uri).cloned()
    }

    fn get_all_keywords(&self) -> HashMap<String, String> {
        let mut all_keywords = HashMap::new();
        for category in self.categories.values() {
            all_keywords.extend(category.items.clone());
        }
        all_keywords
    }
}

fn load_categories() -> HashMap<String, CategoryData> {
    serde_json::from_str(VALUES_JSON).expect("Failed to parse JSON")
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                        SemanticTokensRegistrationOptions {
                            text_document_registration_options: {
                                TextDocumentRegistrationOptions {
                                    document_selector: Some(vec![DocumentFilter {
                                        language: Some("sapf".to_string()),
                                        scheme: Some("file".to_string()),
                                        pattern: None,
                                    }]),
                                }
                            },
                            semantic_tokens_options: SemanticTokensOptions {
                                work_done_progress_options: WorkDoneProgressOptions::default(),
                                legend: SemanticTokensLegend {
                                    token_types: vec![
                                        SemanticTokenType::FUNCTION,
                                        SemanticTokenType::OPERATOR,
                                        SemanticTokenType::NUMBER,
                                    ],
                                    token_modifiers: vec![],
                                },
                                range: Some(true),
                                full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
                            },
                            static_registration_options: StaticRegistrationOptions::default(),
                        },
                    ),
                ),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.documents
            .write()
            .await
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.last() {
            self.documents
                .write()
                .await
                .insert(params.text_document.uri, change.text.clone());
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        if let Some(content) = self.get_document_content(&uri).await {
            let word = get_word_at_position(
                &content,
                position.line as usize,
                position.character as usize,
            );

            if let Some(word) = word {
                if let Some(category) = self.categories.get(word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(
                            category.description.clone(),
                        )),
                        range: None,
                    }));
                }

                let all_keywords = self.get_all_keywords();
                if let Some(doc) = all_keywords.get(word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(doc.clone())),
                        range: None,
                    }));
                }
            }
        }
        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;

        let mut items = Vec::new();

        if let Some(content) = self.get_document_content(&uri).await {
            if let Some(line) = content.lines().nth(position.line as usize) {
                let prefix = &line[..position.character as usize];

                // First, add category completions
                for (category_name, category_data) in &self.categories {
                    if category_name.starts_with(prefix) {
                        items.push(CompletionItem {
                            label: category_name.clone(),
                            kind: Some(CompletionItemKind::MODULE),
                            documentation: Some(Documentation::String(
                                category_data.description.clone(),
                            )),
                            insert_text: Some(format!("{category_name}.")),
                            command: Some(Command {
                                title: "Trigger Suggestion".to_string(),
                                command: "editor.action.triggerSuggest".to_string(),
                                arguments: None,
                            }),
                            ..Default::default()
                        });
                    }
                }

                if let Some((category_prefix, item_prefix)) = prefix.split_once('.') {
                    if let Some(category) = self.categories.get(category_prefix) {
                        items.extend(
                            category
                                .items
                                .iter()
                                .filter(|(k, _)| k.starts_with(item_prefix.trim()))
                                .map(|(k, d)| CompletionItem {
                                    label: k.clone(),
                                    kind: Some(CompletionItemKind::KEYWORD),
                                    documentation: Some(Documentation::String(d.clone())),
                                    insert_text: Some(k.clone()),
                                    ..Default::default()
                                }),
                        );
                    }
                } else {
                    let all_keywords = self.get_all_keywords();
                    items.extend(
                        all_keywords
                            .iter()
                            .filter(|(k, _)| k.starts_with(prefix))
                            .map(|(k, d)| CompletionItem {
                                label: k.clone(),
                                kind: Some(CompletionItemKind::KEYWORD),
                                documentation: Some(Documentation::String(d.clone())),
                                insert_text: Some(k.clone()),
                                ..Default::default()
                            }),
                    );
                }
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        if let Some(content) = self.get_document_content(&uri).await {
            let mut tokens = Vec::new();
            let all_keywords = self.get_all_keywords();

            for (line_num, line) in content.lines().enumerate() {
                let mut offset: u32 = 0;

                let mut chars = line.chars().peekable();
                while let Some(c) = chars.next() {
                    match c {
                        '+' | '-' | '*' | '/' | '=' => {
                            tokens.push(SemanticToken {
                                delta_line: line_num as u32,
                                delta_start: offset,
                                length: 1,
                                token_type: 1,
                                token_modifiers_bitset: 0,
                            });
                            offset += 1;
                        }

                        c if c.is_ascii_digit() => {
                            let mut length: u32 = 1;
                            while let Some(&next_c) = chars.peek() {
                                if next_c.is_ascii_digit() || next_c == '.' {
                                    length += 1;
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            tokens.push(SemanticToken {
                                delta_line: line_num as u32,
                                delta_start: offset,
                                length,
                                token_type: 2,
                                token_modifiers_bitset: 0,
                            });
                            offset += length;
                        }

                        c if c.is_alphabetic() => {
                            let mut word = String::new();
                            word.push(c);
                            while let Some(&next_c) = chars.peek() {
                                if next_c.is_alphanumeric() || next_c == '_' {
                                    word.push(next_c);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            if all_keywords.contains_key(&word) {
                                tokens.push(SemanticToken {
                                    delta_line: line_num as u32,
                                    delta_start: offset,
                                    length: word.len() as u32,
                                    token_type: 0,
                                    token_modifiers_bitset: 0,
                                });
                            }
                            offset += word.len() as u32;
                        }

                        _ => {
                            offset += 1;
                        }
                    }
                }
            }

            let mut absolute_tokens = Vec::new();
            let mut current_line = 0;
            let mut current_start = 0;

            for token in tokens {
                if token.delta_line == current_line {
                    absolute_tokens.push(SemanticToken {
                        delta_line: 0,
                        delta_start: token.delta_start - current_start,
                        ..token
                    });
                } else {
                    absolute_tokens.push(SemanticToken {
                        delta_line: token.delta_line - current_line,
                        delta_start: token.delta_start,
                        ..token
                    });
                }
                current_line = token.delta_line;
                current_start = token.delta_start;
            }

            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: absolute_tokens,
            })));
        }

        Ok(None)
    }
}

fn get_word_at_position(content: &str, line: usize, character: usize) -> Option<&str> {
    let line_content = content.lines().nth(line)?;
    let mut current_pos = 0;

    for word in line_content.split_whitespace() {
        let word_end = current_pos + word.len();

        if character >= current_pos && character <= word_end {
            return Some(word);
        }

        current_pos = word_end + 1;
    }

    None
}

#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
