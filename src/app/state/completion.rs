/// State for the code completion popup.
pub struct CompletionState {
    /// All items received from the server.
    pub items: Vec<CompletionItemEntry>,
    /// Indices into `items` after prefix filtering.
    pub filtered: Vec<usize>,
    /// Currently highlighted row in `filtered`.
    pub selected: usize,
    /// Byte offset where completion was triggered (start of the prefix).
    pub anchor_byte: usize,
    /// Line of the trigger position (for popup positioning).
    #[allow(dead_code)]
    pub anchor_line: usize,
    /// Display column of the trigger position.
    #[allow(dead_code)]
    pub anchor_col: usize,
}

/// A single completion item (simplified from LSP).
pub struct CompletionItemEntry {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub filter_text: String,
    pub kind_label: &'static str,
}

impl CompletionState {
    pub fn new(anchor_byte: usize, anchor_line: usize, anchor_col: usize) -> Self {
        Self {
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            anchor_byte,
            anchor_line,
            anchor_col,
        }
    }

    /// Re-filter items against the typed prefix.
    pub fn filter(&mut self, prefix: &str) {
        let lower_prefix = prefix.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.filter_text.to_lowercase().contains(&lower_prefix))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    /// Get the currently selected item, if any.
    pub fn selected_item(&self) -> Option<&CompletionItemEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.items.get(i))
    }
}

/// State for the hover info popup.
pub struct HoverState {
    pub content: String,
    #[allow(dead_code)]
    pub anchor_line: usize,
    #[allow(dead_code)]
    pub anchor_col: usize,
}
