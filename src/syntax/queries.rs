//! Tree-sitter queries for cross-cutting features.
//!
//! * **Symbols-in-file** (`Ctrl+Shift+O` picker) — each grammar's symbols
//!   query captures `@name` for every named definition and tags the match
//!   with `@symbol.<kind>` so the runtime can render a kind glyph.
//! * **Folds** — `@fold` captures mark every node range that should be
//!   foldable; the buffer's `FoldState` chooses which of those candidate
//!   ranges are currently collapsed.
//!
//! Queries are kept inline as Rust string constants so the grammar list and
//! the queries that exercise it sit in the same place. Each grammar gets a
//! pragmatic best-effort query — the goal is "useful in 90% of files",
//! not "perfect parse-tree coverage".

use crate::syntax::language::Lang;

// ── Symbol queries ────────────────────────────────────────────────────────

const SYMBOLS_RUST: &str = r#"
(function_item name: (identifier) @name) @symbol.fn
(impl_item type: (type_identifier) @name) @symbol.impl
(struct_item name: (type_identifier) @name) @symbol.struct
(enum_item name: (type_identifier) @name) @symbol.enum
(trait_item name: (type_identifier) @name) @symbol.trait
(mod_item name: (identifier) @name) @symbol.mod
(const_item name: (identifier) @name) @symbol.const
(static_item name: (identifier) @name) @symbol.static
(macro_definition name: (identifier) @name) @symbol.macro
(type_item name: (type_identifier) @name) @symbol.type
"#;

const SYMBOLS_PYTHON: &str = r#"
(function_definition name: (identifier) @name) @symbol.fn
(class_definition name: (identifier) @name) @symbol.class
(decorated_definition (function_definition name: (identifier) @name)) @symbol.fn
(decorated_definition (class_definition name: (identifier) @name)) @symbol.class
"#;

const SYMBOLS_JAVASCRIPT: &str = r#"
(function_declaration name: (identifier) @name) @symbol.fn
(method_definition name: (property_identifier) @name) @symbol.fn
(class_declaration name: (identifier) @name) @symbol.class
(variable_declarator name: (identifier) @name value: (arrow_function)) @symbol.fn
(variable_declarator name: (identifier) @name value: (function_expression)) @symbol.fn
(generator_function_declaration name: (identifier) @name) @symbol.fn
"#;

const SYMBOLS_TYPESCRIPT: &str = r#"
(function_declaration name: (identifier) @name) @symbol.fn
(method_definition name: (property_identifier) @name) @symbol.fn
(class_declaration name: (type_identifier) @name) @symbol.class
(interface_declaration name: (type_identifier) @name) @symbol.interface
(type_alias_declaration name: (type_identifier) @name) @symbol.type
(enum_declaration name: (identifier) @name) @symbol.enum
(variable_declarator name: (identifier) @name value: (arrow_function)) @symbol.fn
"#;

const SYMBOLS_TSX: &str = SYMBOLS_TYPESCRIPT;

const SYMBOLS_JSON: &str = r#"
(pair key: (string (string_content) @name)) @symbol.key
"#;

const SYMBOLS_MARKDOWN: &str = r#"
(atx_heading (atx_h1_marker) heading_content: (_) @name) @symbol.h1
(atx_heading (atx_h2_marker) heading_content: (_) @name) @symbol.h2
(atx_heading (atx_h3_marker) heading_content: (_) @name) @symbol.h3
(atx_heading (atx_h4_marker) heading_content: (_) @name) @symbol.h4
"#;

const SYMBOLS_BASH: &str = r#"
(function_definition name: (word) @name) @symbol.fn
"#;

const SYMBOLS_CSHARP: &str = r#"
(class_declaration name: (identifier) @name) @symbol.class
(interface_declaration name: (identifier) @name) @symbol.interface
(struct_declaration name: (identifier) @name) @symbol.struct
(enum_declaration name: (identifier) @name) @symbol.enum
(method_declaration name: (identifier) @name) @symbol.fn
(constructor_declaration name: (identifier) @name) @symbol.fn
(property_declaration name: (identifier) @name) @symbol.property
(namespace_declaration name: (_) @name) @symbol.ns
"#;

const SYMBOLS_JAVA: &str = r#"
(class_declaration name: (identifier) @name) @symbol.class
(interface_declaration name: (identifier) @name) @symbol.interface
(enum_declaration name: (identifier) @name) @symbol.enum
(method_declaration name: (identifier) @name) @symbol.fn
(constructor_declaration name: (identifier) @name) @symbol.fn
"#;

const SYMBOLS_GO: &str = r#"
(function_declaration name: (identifier) @name) @symbol.fn
(method_declaration name: (field_identifier) @name) @symbol.fn
(type_declaration (type_spec name: (type_identifier) @name)) @symbol.type
(const_declaration (const_spec name: (identifier) @name)) @symbol.const
(var_declaration (var_spec name: (identifier) @name)) @symbol.var
"#;

const SYMBOLS_KOTLIN: &str = r#"
(class_declaration (type_identifier) @name) @symbol.class
(function_declaration (simple_identifier) @name) @symbol.fn
(object_declaration (type_identifier) @name) @symbol.object
(property_declaration (variable_declaration (simple_identifier) @name)) @symbol.property
"#;

const SYMBOLS_GROOVY: &str = r#"
(class_declaration name: (identifier) @name) @symbol.class
(function_definition name: (identifier) @name) @symbol.fn
"#;

const SYMBOLS_YAML: &str = r#"
(block_mapping_pair key: (flow_node) @name) @symbol.key
"#;

const SYMBOLS_PROPERTIES: &str = r#"
(property (key) @name) @symbol.key
"#;

const SYMBOLS_TOML: &str = r#"
(table (bare_key) @name) @symbol.table
(table_array_element (bare_key) @name) @symbol.table
(pair (bare_key) @name) @symbol.key
"#;

const SYMBOLS_HTML: &str = r#"
(element (start_tag (tag_name) @name)) @symbol.tag
"#;

const SYMBOLS_CSS: &str = r#"
(rule_set (selectors) @name) @symbol.rule
"#;

// ── Fold queries ──────────────────────────────────────────────────────────

const FOLDS_RUST: &str = r#"
(function_item body: (block) @fold)
(impl_item body: (declaration_list) @fold)
(struct_item body: (field_declaration_list) @fold)
(enum_item body: (enum_variant_list) @fold)
(trait_item body: (declaration_list) @fold)
(mod_item body: (declaration_list) @fold)
(match_expression body: (match_block) @fold)
"#;

const FOLDS_PYTHON: &str = r#"
(function_definition body: (block) @fold)
(class_definition body: (block) @fold)
(if_statement consequence: (block) @fold)
(for_statement body: (block) @fold)
(while_statement body: (block) @fold)
(try_statement body: (block) @fold)
"#;

const FOLDS_JAVASCRIPT: &str = r#"
(function_declaration body: (statement_block) @fold)
(method_definition body: (statement_block) @fold)
(class_body) @fold
(arrow_function body: (statement_block) @fold)
"#;

const FOLDS_TYPESCRIPT: &str = r#"
(function_declaration body: (statement_block) @fold)
(method_definition body: (statement_block) @fold)
(class_body) @fold
(interface_body) @fold
(arrow_function body: (statement_block) @fold)
"#;

const FOLDS_TSX: &str = FOLDS_TYPESCRIPT;

const FOLDS_JSON: &str = r#"
(object) @fold
(array) @fold
"#;

const FOLDS_MARKDOWN: &str = r#"
(section) @fold
(fenced_code_block) @fold
"#;

const FOLDS_BASH: &str = r#"
(function_definition body: (compound_statement) @fold)
(if_statement) @fold
(for_statement) @fold
(while_statement) @fold
(case_statement) @fold
"#;

const FOLDS_CSHARP: &str = r#"
(class_declaration body: (declaration_list) @fold)
(interface_declaration body: (declaration_list) @fold)
(struct_declaration body: (declaration_list) @fold)
(method_declaration body: (block) @fold)
(constructor_declaration body: (block) @fold)
(namespace_declaration body: (declaration_list) @fold)
"#;

const FOLDS_JAVA: &str = r#"
(class_declaration body: (class_body) @fold)
(interface_declaration body: (interface_body) @fold)
(enum_declaration body: (enum_body) @fold)
(method_declaration body: (block) @fold)
(constructor_declaration body: (constructor_body) @fold)
"#;

const FOLDS_GO: &str = r#"
(function_declaration body: (block) @fold)
(method_declaration body: (block) @fold)
(if_statement consequence: (block) @fold)
(for_statement body: (block) @fold)
"#;

const FOLDS_KOTLIN: &str = r#"
(class_body) @fold
(function_body) @fold
(when_expression) @fold
"#;

const FOLDS_GROOVY: &str = r#"
(class_body) @fold
(closure) @fold
"#;

const FOLDS_YAML: &str = r#"
(block_mapping) @fold
(block_sequence) @fold
"#;

const FOLDS_TOML: &str = r#"
(table) @fold
(table_array_element) @fold
(inline_table) @fold
(array) @fold
"#;

const FOLDS_HTML: &str = r#"
(element) @fold
"#;

const FOLDS_CSS: &str = r#"
(rule_set) @fold
(media_statement) @fold
"#;

/// Return the fold query for `lang`. Empty string means "no folds for this
/// grammar"; `None` means the grammar isn't supported at all.
pub fn folds_query_for(lang: Lang) -> Option<&'static str> {
    Some(match lang {
        Lang::Rust => FOLDS_RUST,
        Lang::Python => FOLDS_PYTHON,
        Lang::JavaScript => FOLDS_JAVASCRIPT,
        Lang::TypeScript => FOLDS_TYPESCRIPT,
        Lang::Tsx => FOLDS_TSX,
        Lang::Json => FOLDS_JSON,
        Lang::Markdown => FOLDS_MARKDOWN,
        Lang::Sh => FOLDS_BASH,
        Lang::CSharp => FOLDS_CSHARP,
        Lang::Java => FOLDS_JAVA,
        Lang::Go => FOLDS_GO,
        Lang::Kotlin => FOLDS_KOTLIN,
        Lang::Groovy => FOLDS_GROOVY,
        Lang::Yaml => FOLDS_YAML,
        Lang::Properties => "",
        Lang::Toml => FOLDS_TOML,
        Lang::Html => FOLDS_HTML,
        Lang::Css => FOLDS_CSS,
        Lang::Unknown => return None,
    })
}

/// Return the symbols query for `lang`, or `None` if no query is defined.
pub fn symbols_query_for(lang: Lang) -> Option<&'static str> {
    Some(match lang {
        Lang::Rust => SYMBOLS_RUST,
        Lang::Python => SYMBOLS_PYTHON,
        Lang::JavaScript => SYMBOLS_JAVASCRIPT,
        Lang::TypeScript => SYMBOLS_TYPESCRIPT,
        Lang::Tsx => SYMBOLS_TSX,
        Lang::Json => SYMBOLS_JSON,
        Lang::Markdown => SYMBOLS_MARKDOWN,
        Lang::Sh => SYMBOLS_BASH,
        Lang::CSharp => SYMBOLS_CSHARP,
        Lang::Java => SYMBOLS_JAVA,
        Lang::Go => SYMBOLS_GO,
        Lang::Kotlin => SYMBOLS_KOTLIN,
        Lang::Groovy => SYMBOLS_GROOVY,
        Lang::Yaml => SYMBOLS_YAML,
        Lang::Properties => SYMBOLS_PROPERTIES,
        Lang::Toml => SYMBOLS_TOML,
        Lang::Html => SYMBOLS_HTML,
        Lang::Css => SYMBOLS_CSS,
        Lang::Unknown => return None,
    })
}
