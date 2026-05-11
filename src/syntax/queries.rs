//! Tree-sitter queries for cross-cutting features.
//!
//! Currently houses the **symbols-in-file** queries used by the
//! `Ctrl+Shift+O` picker: each grammar's query captures a `@name` for every
//! named definition (function, struct, class, etc.) and tags it with
//! `@symbol.<kind>` so the runtime can render a kind glyph.
//!
//! Queries are kept inline as Rust string constants so the grammar list and
//! the queries that exercise it sit in the same place. Each grammar gets a
//! pragmatic best-effort query — the goal is "useful in 90% of files",
//! not "perfect parse-tree coverage". Fold queries (driving the code-fold
//! feature) live alongside these constants once that feature is added.

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
