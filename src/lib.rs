use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use itertools::Itertools;
use miette::miette;
use serde::Serialize;

mod category;

use category::{builtin_category, builtin_category_count};

pub use mq_help::{HelpEntry as DocEntry, HelpExample as DocExample, HelpParam as DocParam};

/// Documentation output format.
#[derive(Clone, Debug, Default, clap::ValueEnum)]
pub enum DocFormat {
    #[default]
    Markdown,
    Text,
    Html,
    Json,
}

/// A group of documented functions and selectors belonging to a single module or file.
pub struct ModuleEntry {
    pub name: String,
    pub description: String,
    pub examples: Vec<DocExample>,
    pub functions: Vec<DocEntry>,
    pub selectors: Vec<DocEntry>,
}

/// Whether an entry's description carries mq-hir's own "deprecated" marker convention
/// (a case-insensitive `deprecated` substring) — mirrored here since `mq-help` entries
/// don't carry a dedicated flag.
pub fn is_deprecated(entry: &DocEntry) -> bool {
    entry.description.to_lowercase().contains("deprecated")
}

/// Extract flattened, optionally filtered function and selector entries.
pub fn extract_entries(
    module_names: &Option<Vec<String>>,
    files: &Option<Vec<(String, String)>>,
    include_builtin: bool,
    filter: Option<&str>,
) -> Result<(Vec<DocEntry>, Vec<DocEntry>), miette::Error> {
    let modules = build_modules(module_names, files, include_builtin)?;
    let filter_lower = filter.map(|f| f.to_lowercase());
    let matches = |e: &&DocEntry| {
        filter_lower.as_ref().is_none_or(|q| {
            e.name.to_lowercase().contains(q.as_str()) || e.description.to_lowercase().contains(q.as_str())
        })
    };

    let functions = modules.iter().flat_map(|m| m.functions.iter()).filter(matches).cloned().collect();
    let selectors = modules.iter().flat_map(|m| m.selectors.iter()).filter(matches).cloned().collect();

    Ok((functions, selectors))
}

/// Extract entries grouped by module, without flattening.
pub fn extract_module_entries(
    module_names: &Option<Vec<String>>,
    files: &Option<Vec<(String, String)>>,
    include_builtin: bool,
) -> Result<Vec<ModuleEntry>, miette::Error> {
    build_modules(module_names, files, include_builtin)
}

/// Generate documentation for mq functions, macros, and selectors.
///
/// If `module_names` or `files` is provided, only the specified modules/files are loaded.
/// Both can be combined. If `include_builtin` is true, built-in functions are also included.
/// Otherwise, all builtin functions are documented.
/// An optional `filter` performs case-insensitive substring matching on name and description.
pub fn generate_docs(
    module_names: &Option<Vec<String>>,
    files: &Option<Vec<(String, String)>>,
    format: &DocFormat,
    include_builtin: bool,
    filter: Option<&str>,
) -> Result<String, miette::Error> {
    let mut modules = build_modules(module_names, files, include_builtin)?;

    if let Some(q) = filter {
        let q_lower = q.to_lowercase();
        let keep = |e: &DocEntry| {
            e.name.to_lowercase().contains(&q_lower) || e.description.to_lowercase().contains(&q_lower)
        };
        for m in &mut modules {
            m.functions.retain(keep);
            m.selectors.retain(keep);
        }
    }

    match format {
        DocFormat::Markdown => Ok(format_markdown(&modules)),
        DocFormat::Text => Ok(format_text(&modules)),
        DocFormat::Html => Ok(format_html(&modules)),
        DocFormat::Json => format_json(&modules),
    }
}

/// Build the list of `ModuleEntry` from the given parameters, backed by `mq-help`'s unified
/// documentation catalog (native builtins, selectors, and every standard module), so the
/// same real, verified examples shown by `mq help` are available here.
fn build_modules(
    module_names: &Option<Vec<String>>,
    files: &Option<Vec<(String, String)>>,
    include_builtin: bool,
) -> Result<Vec<ModuleEntry>, miette::Error> {
    let has_files = files.as_ref().is_some_and(|f| !f.is_empty());
    let has_modules = module_names.as_ref().is_some_and(|m| !m.is_empty());

    if !has_files && !has_modules {
        let mut modules = vec![builtin_module()];
        modules.extend(mq_help::all_modules().into_iter().map(standard_module));
        return Ok(modules);
    }

    let mut modules = Vec::new();

    if include_builtin {
        modules.push(builtin_module());
    }

    if let Some(names) = module_names {
        for name in names {
            modules.push(match mq_help::lookup_module(name) {
                Some(m) => standard_module(m),
                None => generic_module(name)?,
            });
        }
    }

    if let Some(file_contents) = files {
        for (filename, content) in file_contents {
            modules.push(file_module(filename, content));
        }
    }

    Ok(modules)
}

/// Native builtins, `builtin.mq` functions, and native selectors.
fn builtin_module() -> ModuleEntry {
    let (functions, selectors) = mq_help::top_level_entries().into_iter().partition(|e| e.kind == "function");
    ModuleEntry {
        name: "Built-in".to_string(),
        description: String::new(),
        examples: Vec::new(),
        functions,
        selectors,
    }
}

fn standard_module(m: mq_help::HelpModule) -> ModuleEntry {
    ModuleEntry {
        name: m.name,
        description: m.description,
        examples: m.examples,
        functions: m.functions,
        selectors: Vec::new(),
    }
}

/// Document an arbitrary `.mq` file by parsing its doc comments the same way `mq-help` does
/// for standard modules, so user-authored files get real (not synthesized) examples too.
fn file_module(filename: &str, content: &str) -> ModuleEntry {
    let (doc, fdocs) = mq_help::extract_module(content, false);
    let functions = fdocs.into_iter().map(|f| from_mq_fn_doc(f, None)).collect();

    ModuleEntry {
        name: filename.to_string(),
        description: doc.as_ref().map(|d| d.description.clone()).unwrap_or_default(),
        examples: doc
            .map(|d| {
                d.examples
                    .into_iter()
                    .map(|e| DocExample { code: e.code, expected: e.expected })
                    .collect()
            })
            .unwrap_or_default(),
        functions,
        selectors: Vec::new(),
    }
}

/// Fallback for a `-M` module name that isn't one of `mq-help`'s compiled-in standard
/// modules — resolved generically via `include "name"`, the same mechanism mq itself uses.
/// Real examples/return types aren't available this way, only name/params/description.
fn generic_module(module_name: &str) -> Result<ModuleEntry, miette::Error> {
    let mut hir = mq_hir::Hir::default();
    hir.builtin.disabled = true;
    hir.add_code(None, &format!("include \"{module_name}\""));

    let source_id = hir.symbols().find_map(|(_, symbol)| {
        if let mq_hir::Symbol { kind: mq_hir::SymbolKind::Include(module_source_id), .. } = &symbol {
            Some(*module_source_id)
        } else {
            None
        }
    });

    let functions = hir
        .symbols()
        .sorted_by_key(|(_, symbol)| symbol.value.clone())
        .filter_map(|(_, symbol)| {
            if let Some(sid) = source_id
                && symbol.source.source_id != Some(sid)
            {
                return None;
            }

            match &symbol {
                mq_hir::Symbol { kind: mq_hir::SymbolKind::Function(params), value: Some(value), doc, .. }
                    if !symbol.is_internal_function() =>
                {
                    Some(DocEntry {
                        name: value.to_string(),
                        kind: "function",
                        params: params
                            .iter()
                            .map(|p| DocParam { name: p.name.to_string(), type_name: "dynamic".to_string() })
                            .collect(),
                        returns: "dynamic".to_string(),
                        description: doc.iter().map(|(_, d)| d.to_string()).join("\n"),
                        examples: Vec::new(),
                        capability: None,
                        related_module: Some(module_name.to_string()),
                        aliases: Vec::new(),
                    })
                }
                _ => None,
            }
        })
        .collect();

    Ok(ModuleEntry {
        name: module_name.to_string(),
        description: String::new(),
        examples: Vec::new(),
        functions,
        selectors: Vec::new(),
    })
}

fn from_mq_fn_doc(f: mq_help::MqFnDoc, related_module: Option<String>) -> DocEntry {
    DocEntry {
        name: f.name,
        kind: "function",
        params: f.params.into_iter().map(|name| DocParam { name, type_name: "dynamic".to_string() }).collect(),
        returns: f.returns.unwrap_or_else(|| "dynamic".to_string()),
        description: f.description,
        examples: f.examples.into_iter().map(|e| DocExample { code: e.code, expected: e.expected }).collect(),
        capability: None,
        related_module,
        aliases: Vec::new(),
    }
}

/// Format documentation as Markdown, reusing `mq-help`'s own per-entry renderer so mq-docs
/// and `mq help --markdown` can never drift apart on how a function/example is shown.
fn format_markdown(modules: &[ModuleEntry]) -> String {
    let mut out = String::new();

    for m in modules {
        if m.functions.is_empty() && m.selectors.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }

        let _ = writeln!(out, "# {}", m.name);
        if !m.description.is_empty() {
            let _ = writeln!(out, "\n{}", m.description);
        }
        for example in &m.examples {
            let _ = writeln!(out, "\n```mq\n{}\n```\n\nOutput:\n\n```\n{}\n```", example.code, example.expected);
        }

        for entry in m.functions.iter().chain(m.selectors.iter()) {
            out.push('\n');
            out.push_str(&mq_help::render_markdown(entry));
        }
    }

    out
}

/// Format documentation as plain text: one block per entry with its signature, description,
/// and any verified examples — the previous fixed-width table couldn't hold multi-line,
/// possibly-multiple examples, so this format was restructured to make room for them.
fn format_text(modules: &[ModuleEntry]) -> String {
    let mut out = String::new();

    for m in modules {
        if m.functions.is_empty() && m.selectors.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }

        let _ = writeln!(out, "# {}", m.name);
        if !m.description.is_empty() {
            let _ = writeln!(out, "\n{}", m.description);
        }
        for example in &m.examples {
            let _ = writeln!(out, "\n  {}\n  #=> {}", example.code, example.expected);
        }

        if !m.functions.is_empty() {
            let _ = writeln!(out, "\n## Functions ({})\n", m.functions.len());
            for f in &m.functions {
                out.push_str(&render_entry_text(f));
            }
        }
        if !m.selectors.is_empty() {
            let _ = writeln!(out, "\n## Selectors ({})\n", m.selectors.len());
            for s in &m.selectors {
                out.push_str(&render_entry_text(s));
            }
        }
    }

    out
}

fn render_entry_text(e: &DocEntry) -> String {
    let mut out = String::new();
    let deprecated = if is_deprecated(e) { " [deprecated]" } else { "" };

    let _ = writeln!(out, "{}{deprecated}", signature_text(e));
    if !e.description.is_empty() {
        let _ = writeln!(out, "  {}", e.description);
    }
    if !e.aliases.is_empty() {
        let _ = writeln!(out, "  Aliases: {}", e.aliases.join(", "));
    }
    if let Some(module) = &e.related_module {
        let _ = writeln!(out, "  Module: import \"{module}\" | {module}::{}(...)", e.name);
    }
    if let Some(cap) = &e.capability {
        let _ = writeln!(out, "  Capability: requires `{cap}` (not available via the hosted Web API/playground)");
    }
    for example in &e.examples {
        let _ = writeln!(out, "  Example:");
        let _ = writeln!(out, "    {}", example.code.replace('\n', "\n    "));
        let _ = writeln!(out, "    #=> {}", example.expected.replace('\n', "\n    "));
    }
    out.push('\n');

    out
}

/// A documentation entry plus its computed deprecation flag, for JSON output.
#[derive(Serialize)]
struct JsonEntry<'a> {
    #[serde(flatten)]
    entry: &'a DocEntry,
    is_deprecated: bool,
}

#[derive(Serialize)]
struct JsonModule<'a> {
    name: &'a str,
    description: &'a str,
    examples: &'a [DocExample],
    functions: Vec<JsonEntry<'a>>,
    selectors: Vec<JsonEntry<'a>>,
}

/// Format documentation as JSON: an array of modules, each with its own description/examples
/// plus structured function and selector records (params, return type, examples, capability).
fn to_json_entries(entries: &[DocEntry]) -> Vec<JsonEntry<'_>> {
    entries.iter().map(|e| JsonEntry { entry: e, is_deprecated: is_deprecated(e) }).collect()
}

fn format_json(modules: &[ModuleEntry]) -> Result<String, miette::Error> {
    let json_modules: Vec<JsonModule> = modules
        .iter()
        .map(|m| JsonModule {
            name: &m.name,
            description: &m.description,
            examples: &m.examples,
            functions: to_json_entries(&m.functions),
            selectors: to_json_entries(&m.selectors),
        })
        .collect();

    serde_json::to_string_pretty(&json_modules).map_err(|e| miette!("{e}"))
}

/// Lowercase, hyphen-separated form of `s` used inside page ids and permalinks.
fn slugify(s: &str) -> String {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|part| !part.is_empty()).join("-")
}

/// Reserve `base` in `used`, appending `-2`, `-3`, ... when it is already taken.
fn unique_id(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| used.insert(candidate.clone()))
        .expect("an unbounded range always yields a free id")
}

/// Percent-encode a permalink so it survives as a URL fragment (`<`, `[`, `$`, ... in selector names).
fn href_fragment(anchor: &str) -> String {
    let mut out = String::from("#");
    for b in anchor.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Sort key that ignores leading punctuation, so `.h1` sits next to `h1` rather than before `a`.
fn entry_sort_key(name: &str) -> String {
    name.trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Jump-bar bucket of an entry: its first letter, or `*` for digits and symbols.
fn entry_letter(name: &str) -> char {
    name.chars().find(|c| c.is_alphanumeric()).filter(char::is_ascii_alphabetic).map_or('*', |c| c.to_ascii_uppercase())
}

/// An entry together with its permalink, as laid out on the HTML page.
struct HtmlItem<'a> {
    entry: &'a DocEntry,
    anchor: String,
    module: usize,
    module_name: &'a str,
    /// Whether the entry belongs to the native built-ins group.
    builtin: bool,
    /// Heading the entry is filed under on a grouped page, and that heading's display order.
    group: String,
    rank: usize,
}

/// Page ids and permalinks for one module.
struct ModuleLayout<'a> {
    page_id: String,
    selector_page_id: String,
    functions: Vec<HtmlItem<'a>>,
    selectors: Vec<HtmlItem<'a>>,
}

/// Sort `entries` and give each a permalink that is unique across the whole page. Built-in
/// functions are filed under their category; other modules' functions under the module name.
fn layout_entries<'a>(
    module: &'a ModuleEntry,
    idx: usize,
    entries: &'a [DocEntry],
    prefix: Option<&str>,
    used: &mut HashSet<String>,
) -> Vec<HtmlItem<'a>> {
    let builtin = is_builtin_name(&module.name);
    let mut sorted: Vec<&DocEntry> = entries.iter().collect();
    sorted.sort_by_cached_key(|e| (entry_sort_key(&e.name), e.name.clone()));
    sorted
        .into_iter()
        .map(|entry| {
            let base = prefix.map_or_else(|| entry.name.clone(), |p| format!("{p}::{}", entry.name));
            let (rank, group) = match (builtin, entry.kind) {
                (true, "function") => {
                    let (rank, title) = builtin_category(&entry.name);
                    (rank, title.to_string())
                }
                _ => (builtin_category_count() + idx, module.name.clone()),
            };
            HtmlItem {
                entry,
                anchor: unique_id(base, used),
                module: idx,
                module_name: &module.name,
                builtin,
                group,
                rank,
            }
        })
        .collect()
}

/// Assign page ids and unique permalinks to every module and entry. Built-in entries are
/// addressed as `#abs`, everything else as `#csv::csv_parse`.
fn layout_modules(modules: &[ModuleEntry]) -> Vec<ModuleLayout<'_>> {
    let mut used: HashSet<String> = HashSet::from(["mod-all".to_string(), "search-results".to_string()]);
    let page_ids: Vec<(String, String)> = modules
        .iter()
        .map(|m| {
            let slug = slugify(&m.name);
            (unique_id(format!("mod-{slug}"), &mut used), unique_id(format!("sel-{slug}"), &mut used))
        })
        .collect();

    let mut layouts = Vec::new();
    for (i, (m, (page_id, selector_page_id))) in modules.iter().zip(page_ids).enumerate() {
        let prefix = (!is_builtin_name(&m.name)).then(|| slugify(&m.name));
        layouts.push(ModuleLayout {
            page_id,
            selector_page_id,
            functions: layout_entries(m, i, &m.functions, prefix.as_deref(), &mut used),
            selectors: layout_entries(m, i, &m.selectors, prefix.as_deref(), &mut used),
        });
    }
    layouts
}

/// Where a function name can be linked to.
struct LinkTarget {
    module: usize,
    builtin: bool,
    anchor: String,
}

/// Name → permalink lookup used to cross-link function names in descriptions and examples.
struct LinkIndex {
    by_name: HashMap<String, Vec<LinkTarget>>,
    anchors: HashSet<String>,
}

impl LinkIndex {
    fn new(modules: &[ModuleEntry], layouts: &[ModuleLayout<'_>]) -> Self {
        let mut index = LinkIndex { by_name: HashMap::new(), anchors: HashSet::new() };
        for (m, layout) in modules.iter().zip(layouts) {
            let builtin = is_builtin_name(&m.name);
            for item in layout.functions.iter().chain(&layout.selectors) {
                index.anchors.insert(item.anchor.clone());
                let names = std::iter::once(&item.entry.name).chain(&item.entry.aliases);
                for name in names {
                    index.by_name.entry(name.clone()).or_default().push(LinkTarget {
                        module: item.module,
                        builtin,
                        anchor: item.anchor.clone(),
                    });
                }
            }
        }
        index
    }

    /// Resolve a bare name seen in module `from`: its own module wins, then the built-ins.
    /// Other modules are only reachable through `module::name`, since a bare `` `text` ``
    /// usually names a node type or argument rather than that module's function.
    fn resolve(&self, name: &str, from: usize) -> Option<&str> {
        let candidates = self.by_name.get(name)?;
        candidates
            .iter()
            .find(|t| t.module == from)
            .or_else(|| candidates.iter().find(|t| t.builtin))
            .map(|t| t.anchor.as_str())
    }
}

/// Everything needed to cross-link names inside one entry's text.
struct LinkCtx<'a> {
    index: &'a LinkIndex,
    module: usize,
    self_anchor: &'a str,
    params: &'a [DocParam],
}

impl LinkCtx<'_> {
    /// Permalink for `name`, unless it is the entry itself or one of its own parameters
    /// (`` `path` `` in a description usually means the argument, not a function of that name).
    fn target(&self, name: &str) -> Option<&str> {
        if self.params.iter().any(|p| p.name == name) {
            return None;
        }
        let anchor = if name.contains("::") {
            self.index.anchors.get(name).map(String::as_str)
        } else {
            self.index.resolve(name, self.module)
        };
        anchor.filter(|a| *a != self.self_anchor)
    }

    fn link(&self, anchor: &str, inner: &str) -> String {
        format!("<a class=\"xref\" href=\"{}\">{inner}</a>", href_fragment(anchor))
    }
}

/// Render description text: `` `code` `` spans become `<code>`, and spans naming another
/// function (`` `base64d` ``, `` `env(name)` ``) link to it.
fn render_description_html(text: &str, ctx: &LinkCtx<'_>) -> String {
    let parts: Vec<&str> = text.split('`').collect();
    if parts.len().is_multiple_of(2) {
        // Unbalanced backticks: leave the text exactly as written.
        return escape_html(text);
    }

    parts
        .iter()
        .enumerate()
        .map(|(i, part)| if i % 2 == 0 { escape_html(part) } else { render_code_span_html(part, ctx) })
        .collect()
}

fn render_code_span_html(span: &str, ctx: &LinkCtx<'_>) -> String {
    if let Some(anchor) = ctx.target(span) {
        return ctx.link(anchor, &format!("<code>{}</code>", escape_html(span)));
    }

    let ident_len = span.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(span.len());
    let (ident, rest) = span.split_at(ident_len);
    if let Some(anchor) = ctx.target(ident).filter(|_| !ident.is_empty() && rest.starts_with('(')) {
        return format!("<code>{}{}</code>", ctx.link(anchor, &escape_html(ident)), escape_html(rest));
    }

    format!("<code>{}</code>", escape_html(span))
}

/// Words the mq lexer treats as keywords.
const KEYWORDS: &[&str] = &[
    "as", "break", "catch", "continue", "def", "do", "elif", "else", "end", "fn", "foreach", "if", "import", "include",
    "let", "loop", "match", "module", "nodes", "self", "try", "unless", "until", "var", "while", "yield",
];

/// Literal values (`true`, `false`, `None`).
const CONSTANTS: &[&str] = &["true", "false", "None"];

fn token_html(class: &str, text: &str) -> String {
    format!("<span class=\"{class}\">{}</span>", escape_html(text))
}

/// Highlight example code with a small tokenizer over mq's lexical rules (strings, comments,
/// numbers, keywords, selectors, calls). With a link context, `name(` calls to a documented
/// function also link to it, except `def`/`macro` definitions.
fn render_code_html(code: &str, ctx: Option<&LinkCtx<'_>>) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let text = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let mut out = String::new();
    let mut prev_word = String::new();
    let mut i = 0;

    while i < n {
        let c = chars[i];
        let start = i;
        let interpolated = c == 's' && chars.get(i + 1) == Some(&'"');

        if c == '"' || interpolated {
            i += if interpolated { 2 } else { 1 };
            while i < n && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(n);
            out.push_str(&token_html("tok-str", &text(start, i)));
        } else if c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            out.push_str(&token_html("tok-com", &text(start, i)));
        } else if c.is_ascii_digit() {
            while i < n && is_ident(chars[i]) {
                i += 1;
            }
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                while i < n && is_ident(chars[i]) {
                    i += 1;
                }
            }
            out.push_str(&token_html("tok-num", &text(start, i)));
        } else if c == '.' && chars.get(i + 1).is_some_and(|&d| is_ident(d) || matches!(d, '-' | '*' | '[' | '.' | '<' | '>' | '^')) {
            // Selector: `.h1`, `..`, `.[]`, `.[][]`, `.<>`
            i += 1;
            match chars[i] {
                '.' | '>' | '^' => i += 1,
                '<' => {
                    i += 1;
                    if chars.get(i) == Some(&'>') {
                        i += 1;
                    }
                }
                '[' => {
                    while i < n && matches!(chars[i], '[' | ']') {
                        i += 1;
                    }
                }
                _ => {
                    while i < n && (is_ident(chars[i]) || matches!(chars[i], '-' | '*')) {
                        i += 1;
                    }
                }
            }
            out.push_str(&token_html("tok-sel", &text(start, i)));
            prev_word.clear();
        } else if c.is_alphabetic() || c == '_' {
            while i < n && is_ident(chars[i]) {
                i += 1;
            }
            // `module::name`
            if chars[i..].starts_with(&[':', ':']) && chars.get(i + 2).is_some_and(|&c| c.is_alphabetic() || c == '_') {
                i += 2;
                while i < n && is_ident(chars[i]) {
                    i += 1;
                }
            }

            let word = text(start, i);
            let is_call = chars.get(i) == Some(&'(');
            let is_definition = matches!(prev_word.as_str(), "def" | "macro");

            if KEYWORDS.contains(&word.as_str()) {
                out.push_str(&token_html("tok-kw", &word));
            } else if CONSTANTS.contains(&word.as_str()) {
                out.push_str(&token_html("tok-const", &word));
            } else if is_call {
                let anchor = ctx.filter(|_| !is_definition).and_then(|ctx| ctx.target(&word).map(|a| (ctx, a)));
                match anchor {
                    Some((ctx, anchor)) => out.push_str(&ctx.link(anchor, &token_html("tok-fn", &word))),
                    None => out.push_str(&token_html("tok-fn", &word)),
                }
            } else {
                out.push_str(&escape_html(&word));
            }
            prev_word = word;
        } else {
            i += 1;
            if !c.is_whitespace() {
                prev_word.clear();
            }
            out.push_str(&escape_html(&c.to_string()));
        }
    }

    out
}

/// Render one labeled example: a "Example"-labeled code block for the query, paired with a
/// "Result"-labeled block for its verified output — symmetric labels rather than a floating
/// arrow glyph, which renders inconsistently across fonts and is fragile to align.
fn render_example_html(example: &DocExample, label: &str, ctx: Option<&LinkCtx<'_>>) -> String {
    let code = render_code_html(&example.code, ctx);
    format!(
        "<div class=\"example\">\n\
         \x20 <div class=\"example-label\">{}</div>\n\
         \x20 <pre class=\"example-code\"><code>{code}</code></pre>\n\
         \x20 <div class=\"example-label\">Result</div>\n\
         \x20 <pre class=\"example-result\"><code>{}</code></pre>\n\
         </div>",
        escape_html(label),
        escape_html(&example.expected)
    )
}

/// "Example" for a single example, "Example 1"/"Example 2"/... when a function documents
/// several — numbering only appears when it's actually meaningful.
fn render_examples_html(examples: &[DocExample], ctx: Option<&LinkCtx<'_>>) -> String {
    examples
        .iter()
        .enumerate()
        .map(|(i, ex)| {
            let label = if examples.len() > 1 { format!("Example {}", i + 1) } else { "Example".to_string() };
            render_example_html(ex, &label, ctx)
        })
        .join("\n")
}

/// Parameters as written in a signature. A type is shown only when it says something:
/// `dynamic` (what mq-help reports when it has no real type) is left out as noise.
fn signature_params(entry: &DocEntry) -> String {
    entry
        .params
        .iter()
        .map(|p| if p.type_name == "dynamic" { p.name.clone() } else { format!("{}: {}", p.name, p.type_name) })
        .join(", ")
}

/// Return type as written in a signature, `None` when it is only `dynamic`.
fn signature_returns(entry: &DocEntry) -> Option<&str> {
    Some(entry.returns.as_str()).filter(|r| *r != "dynamic" && !r.is_empty())
}

/// Plain-text signature, e.g. `add(value1, value2)` or `abs(number: number): number`.
fn signature_text(entry: &DocEntry) -> String {
    let returns = signature_returns(entry).map(|r| format!(": {r}")).unwrap_or_default();
    format!("{}({}){returns}", entry.name, signature_params(entry))
}

/// Split a description into its body and the sentence announcing the deprecation
/// ("Deprecated: use has_body() instead."), so the notice can be shown as a warning.
fn split_deprecation(description: &str) -> (String, Option<String>) {
    let Some(at) = description.to_ascii_lowercase().find("deprecated") else {
        return (description.to_string(), None);
    };

    let head = &description[..at];
    let start = head.rfind(". ").map(|i| i + 2).into_iter().chain(head.rfind('\n').map(|i| i + 1)).max().unwrap_or(0);
    let end = description[at..]
        .find(". ")
        .map(|i| at + i + 1)
        .or_else(|| description[at..].find('\n').map(|i| at + i))
        .unwrap_or(description.len());

    let note = description[start..end].trim().to_string();
    let body = format!("{} {}", description[..start].trim(), description[end..].trim()).trim().to_string();
    (body, Some(note))
}

const REPO_URL: &str = "https://github.com/harehare/mq";

/// Version of mq whose docs and sources are shown (the tag source links point at).
fn mq_version() -> &'static str {
    mq_lang::Engine::<mq_lang::DefaultModuleResolver>::version()
}

/// Link to a standard module's `.mq` source in the `mq` repository, at the tag matching the
/// docs shown. Built-ins have none: they are split between `builtin.mq` and native Rust, and
/// a user-supplied file has no upstream source to point at.
fn render_module_source_html(module_name: &str) -> String {
    if !mq_lang::STANDARD_MODULES.contains_key(module_name) {
        return String::new();
    }

    let path = format!("crates/mq-lang/modules/{module_name}.mq");
    format!(
        "<p class=\"module-source\">Source: \
         <a href=\"{REPO_URL}/blob/v{}/{path}\" target=\"_blank\" rel=\"noopener\">{module_name}.mq</a></p>",
        mq_version()
    )
}

/// Render a single function/selector as a self-contained card: signature (its name is the permalink), badges,
/// deprecation warning, aliases, description, related-module usage and every verified example.
/// Each entry is rendered once, with its permalink `id`; the "All" page clones it in the browser.
/// `group` is the jump-bar bucket (a letter or a category).
fn render_entry_html(item: &HtmlItem<'_>, group: &str, index: &LinkIndex) -> String {
    let entry = item.entry;
    let ctx = LinkCtx { index, module: item.module, self_anchor: &item.anchor, params: &entry.params };

    let returns = signature_returns(entry).map(|r| format!(": {}", escape_html(r))).unwrap_or_default();
    // The name is the permalink: clicking it selects the entry's URL.
    let sig = format!(
        "<a class=\"entry-name entry-anchor\" href=\"{}\">{}</a>({}){returns}",
        href_fragment(&item.anchor),
        escape_html(&entry.name),
        escape_html(&signature_params(entry))
    );

    let is_selector = entry.kind == "selector";
    let kind_label = if is_selector { "selector" } else { "function" };
    let kind_class = if is_selector { "badge-kind-selector" } else { "badge-kind-function" };
    let deprecated = is_deprecated(entry);
    let mut badges = format!("<span class=\"badge {kind_class}\">{kind_label}</span>");
    if deprecated {
        badges.push_str("<span class=\"badge badge-deprecated\">Deprecated</span>");
    }
    if let Some(cap) = &entry.capability {
        badges.push_str(&format!(
            "<span class=\"badge badge-capability\" title=\"Not available via the hosted Web API/playground\">requires {}</span>",
            escape_html(cap)
        ));
    }

    let (body, note) = if deprecated { split_deprecation(&entry.description) } else { (entry.description.clone(), None) };
    let warning_html = note
        .map(|note| {
            format!(
                "<div class=\"entry-warning\" role=\"note\"><strong>Deprecated</strong><span>{}</span></div>",
                render_description_html(&note, &ctx)
            )
        })
        .unwrap_or_else(|| {
            if deprecated {
                "<div class=\"entry-warning\" role=\"note\"><strong>Deprecated</strong></div>".to_string()
            } else {
                String::new()
            }
        });

    let anchor = escape_html(&item.anchor);
    let class = if deprecated { "entry entry-deprecated" } else { "entry" };
    let aliases_html = if entry.aliases.is_empty() {
        String::new()
    } else {
        let aliases = entry.aliases.iter().map(|a| format!("<code>{}</code>", escape_html(a))).join(" ");
        format!("<p class=\"entry-aliases\">Aliases: {aliases}</p>")
    };

    let description_html = if body.is_empty() {
        String::new()
    } else {
        format!("<p class=\"entry-desc\">{}</p>", render_description_html(&body, &ctx))
    };

    let module_html = entry
        .related_module
        .as_ref()
        .map(|m| {
            format!(
                "<p class=\"entry-module\">Module: <code>import \"{0}\" | {0}::{1}(...)</code></p>",
                escape_html(m),
                escape_html(&entry.name)
            )
        })
        .unwrap_or_default();

    let examples_html = if entry.examples.is_empty() {
        String::new()
    } else {
        format!("<div class=\"entry-examples\">{}</div>", render_examples_html(&entry.examples, Some(&ctx)))
    };

    // Plain-text facts the page script searches and filters on.
    let data = format!(
        "data-anchor=\"{anchor}\" data-group=\"{}\" data-kind=\"{kind_label}\" data-module=\"{}\" data-name=\"{}\" \
         data-aliases=\"{}\" data-desc=\"{}\" data-cap=\"{}\" data-dep=\"{}\"",
        escape_html(group),
        escape_html(item.module_name),
        escape_html(&entry.name),
        escape_html(&entry.aliases.join(" ")),
        escape_html(&entry.description),
        escape_html(entry.capability.as_deref().unwrap_or("")),
        u8::from(deprecated),
    );

    format!(
        "<div class=\"{class}\" id=\"{anchor}\" {data}>\n\
         \x20 <div class=\"entry-header\">\n\
         \x20   <code class=\"entry-sig\">{sig}</code>\n\
         \x20   {badges}\n\
         \x20 </div>\n\
         {warning_html}{aliases_html}{description_html}{module_html}{examples_html}\n\
         </div>"
    )
}

/// Render a module's own header doc (description + usage examples) and source link, shown
/// above its entries.
fn render_module_intro_html(m: &ModuleEntry) -> String {
    let source = render_module_source_html(&m.name);
    if m.description.is_empty() && m.examples.is_empty() && source.is_empty() {
        return String::new();
    }

    let desc = if m.description.is_empty() {
        String::new()
    } else {
        format!("<p class=\"module-desc\">{}</p>", escape_html(&m.description))
    };
    let examples = if m.examples.is_empty() {
        String::new()
    } else {
        format!("<div class=\"module-examples\">{}</div>", render_examples_html(&m.examples, None))
    };

    format!("<div class=\"module-intro\">{desc}{source}{examples}</div>")
}

/// Jump bar for an A–Z page: one link per letter that has an entry.
fn render_jump_bar_html(groups: &[String], label: &str) -> String {
    let links = groups
        .iter()
        .map(|g| {
            let title = if g == "*" { " title=\"Digits and symbols\"" } else { "" };
            format!("<a href=\"#\" data-group=\"{0}\"{title}>{0}</a>", escape_html(g))
        })
        .join("");
    format!("<nav class=\"az-nav\" aria-label=\"{label}\">{links}</nav>")
}

/// Filter chips: `kinds` adds function/selector, the others only appear when the page has
/// at least one such entry to filter on.
fn render_chips_html(kinds: bool, capability: bool, deprecated: bool) -> String {
    let mut chips = Vec::new();
    if kinds {
        chips.push(("kind:function", "Functions"));
        chips.push(("kind:selector", "Selectors"));
    }
    if capability {
        chips.push(("cap", "Requires capability"));
    }
    if deprecated {
        chips.push(("dep", "Deprecated"));
    }
    if chips.is_empty() {
        return String::new();
    }

    let buttons = chips
        .iter()
        .map(|(filter, label)| {
            format!("<button type=\"button\" class=\"chip\" data-filter=\"{filter}\" aria-pressed=\"false\">{label}</button>")
        })
        .join("");
    format!("<div class=\"chips\" role=\"group\" aria-label=\"Filters\">{buttons}</div>")
}

/// How a page is rendered.
struct PageOptions<'a> {
    id: &'a str,
    kind_plural: &'a str,
    intro: &'a str,
    /// Whether the page holds its entries' cards. The "All" page does not: it only lists which
    /// entries go where, and the script clones the cards from the module pages.
    own_cards: bool,
    active: bool,
    is_selector: bool,
}

/// The container for one run of entries. Without cards it records the anchors and group so
/// the script can fill it in.
fn entries_box_html(group: &str, members: &[&HtmlItem<'_>], own_cards: bool, index: &LinkIndex) -> String {
    if own_cards {
        let cards = members.iter().map(|m| render_entry_html(m, group, index)).join("\n");
        format!("<div class=\"entries\">\n{cards}\n</div>")
    } else {
        let anchors = members.iter().map(|m| escape_html(&m.anchor)).join(" ");
        format!("<div class=\"entries\" data-group=\"{}\" data-anchors=\"{anchors}\"></div>", escape_html(group))
    }
}

/// Build one sidebar-linked page listing every entry (a module's functions or selectors).
/// Pages holding built-ins are split under category headings; the rest are listed A–Z.
fn build_entries_page(opts: &PageOptions<'_>, items: &[&HtmlItem<'_>], index: &LinkIndex) -> String {
    let grouped = !opts.is_selector && items.iter().any(|item| item.builtin);

    // Consecutive runs of entries sharing a group: a category, or a letter on A–Z pages.
    let mut sorted = items.to_vec();
    if grouped {
        sorted.sort_by_cached_key(|item| (item.rank, entry_sort_key(&item.entry.name), item.entry.name.clone()));
    }
    let mut sections: Vec<(String, Vec<&HtmlItem<'_>>)> = Vec::new();
    for item in sorted {
        let group = if grouped { item.group.clone() } else { entry_letter(&item.entry.name).to_string() };
        match sections.last_mut() {
            Some((last, members)) if *last == group => members.push(item),
            _ => sections.push((group, vec![item])),
        }
    }
    let groups: Vec<String> = sections.iter().map(|(g, _)| g.clone()).collect();

    let rows = if grouped {
        sections
            .iter()
            .map(|(group, members)| {
                // A module's own heading (on the "All" page) is already a sidebar link.
                let module_class = if members[0].rank >= builtin_category_count() { " entry-group-module" } else { "" };
                format!(
                    "<section class=\"entry-group{module_class}\">\n\
                     <h2 class=\"group-title\">{}<span class=\"group-count\">{}</span></h2>\n\
                     {}\n\
                     </section>",
                    escape_html(group),
                    members.len(),
                    entries_box_html(group, members, opts.own_cards, index)
                )
            })
            .join("\n")
    } else if opts.own_cards {
        let cards = sections
            .iter()
            .flat_map(|(group, members)| members.iter().map(move |m| render_entry_html(m, group, index)))
            .join("\n");
        format!("<div class=\"entries\">\n{cards}\n</div>")
    } else {
        sections.iter().map(|(group, members)| entries_box_html(group, members, false, index)).join("\n")
    };

    // Categories are listed under the page's sidebar link (built by the script); only the
    // A–Z pages need a bar of their own.
    let jump_bar = if grouped { String::new() } else { render_jump_bar_html(&groups, "Jump to letter") };
    let chips = render_chips_html(
        false,
        items.iter().any(|item| item.entry.capability.is_some()),
        items.iter().any(|item| is_deprecated(item.entry)),
    );
    let PageOptions { id, kind_plural, intro, .. } = opts;
    let count = items.len();
    let active_class = if opts.active { " active" } else { "" };
    let grouped_class = if grouped { " grouped" } else { "" };
    format!(
        "<div class=\"module-page{active_class}{grouped_class}\" id=\"{id}\">\n\
         {intro}\n\
         \x20 <div class=\"page-toolbar\">\n\
         \x20   <div class=\"search-box\">\n\
         \x20     <svg class=\"search-icon\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"11\" cy=\"11\" r=\"8\"/><line x1=\"21\" y1=\"21\" x2=\"16.65\" y2=\"16.65\"/></svg>\n\
         \x20     <input type=\"text\" class=\"search-input\" placeholder=\"Filter {kind_plural} on this page...\" aria-label=\"Filter {kind_plural} on this page\" />\n\
         \x20   </div>\n\
         \x20   {chips}\n\
         \x20   {jump_bar}\n\
         \x20 </div>\n\
         \x20 <p class=\"count\"><span class=\"count-num\">{count}</span> {kind_plural}</p>\n\
         {rows}\n\
         \x20 <p class=\"no-results\" hidden>No results</p>\n\
         </div>"
    )
}

/// The page that shows results of the sidebar's search across every module.
fn build_search_page() -> String {
    let chips = render_chips_html(true, true, true);
    format!(
        "<div class=\"module-page search-page\" id=\"search-results\">\n\
         \x20 <div class=\"page-toolbar\">\n\
         \x20   {chips}\n\
         \x20 </div>\n\
         \x20 <p class=\"count\"><span class=\"count-num\">0</span> results for &ldquo;<span class=\"search-term\"></span>&rdquo;</p>\n\
         \x20 <div class=\"results\" role=\"listbox\" aria-label=\"Search results\"></div>\n\
         \x20 <p class=\"no-results\" hidden>No results</p>\n\
         </div>"
    )
}

/// One sidebar link; its `href` is the page id so the URL hash selects the page.
fn sidebar_link(page_id: &str, icon: &str, name: &str, count: usize, active: bool) -> String {
    let active_class = if active { " active" } else { "" };
    format!(
        "<a class=\"sidebar-link{active_class}\" href=\"#{page_id}\" data-module=\"{page_id}\">\
         <span class=\"sidebar-icon\">{icon}</span>\
         <span class=\"sidebar-label\">{}</span>\
         <span class=\"sidebar-count\">{count}</span></a>\n",
        escape_html(name)
    )
}

/// Links to the rest of the mq site, shown in the sidebar header.
fn site_links_html() -> String {
    [
        ("mqlang.org", "https://mqlang.org/"),
        ("Playground", "https://mqlang.org/playground"),
        ("GitHub", REPO_URL),
        ("Book", "https://mqlang.org/book/"),
        ("Cookbook", "https://mqlang.org/book/cookbook"),
    ]
    .iter()
    .map(|(label, url)| format!("<a href=\"{url}\" target=\"_blank\" rel=\"noopener\">{label}</a>"))
    .join("")
}

/// Format documentation as a single-page HTML with sidebar navigation.
///
/// The URL is the router: `#mod-csv` opens a module page, `#csv::csv_parse` (or `#abs` for
/// built-ins) opens the page holding that entry and scrolls to it, and `?q=` keeps the
/// sidebar's cross-module search.
fn format_html(modules: &[ModuleEntry]) -> String {
    let has_multiple = modules.len() > 1;
    let has_selectors = modules.iter().any(|m| !m.selectors.is_empty());

    let layouts = layout_modules(modules);
    let index = LinkIndex::new(modules, &layouts);

    let sidebar_items = if has_multiple {
        let all_count: usize = modules.iter().map(|m| m.functions.len()).sum();
        let all_icon = svg_icon(
            "<rect x=\"3\" y=\"3\" width=\"7\" height=\"7\"/>\
             <rect x=\"14\" y=\"3\" width=\"7\" height=\"7\"/>\
             <rect x=\"3\" y=\"14\" width=\"7\" height=\"7\"/>\
             <rect x=\"14\" y=\"14\" width=\"7\" height=\"7\"/>",
        );
        let mut items = sidebar_link("mod-all", &all_icon, "All", all_count, true);
        for (m, layout) in modules.iter().zip(&layouts) {
            items.push_str(&sidebar_link(&layout.page_id, &module_icon(&m.name), &m.name, m.functions.len(), false));
        }
        items
    } else {
        let m = &modules[0];
        sidebar_link("mod-all", &module_icon(&m.name), &m.name, m.functions.len(), true)
    };

    let selector_sidebar_items: String = modules
        .iter()
        .zip(&layouts)
        .filter(|(m, _)| !m.selectors.is_empty())
        .map(|(m, layout)| sidebar_link(&layout.selector_page_id, &selector_icon(), &m.name, m.selectors.len(), false))
        .collect();

    let function_page = |id: &str, intro: &str, items: &[&HtmlItem<'_>], own_cards: bool, active: bool| {
        let opts = PageOptions { id, kind_plural: "functions", intro, own_cards, active, is_selector: false };
        build_entries_page(&opts, items, &index)
    };

    let mut pages = if has_multiple {
        // The "All" page repeats every entry, merged and sorted; the per-module pages own the ids.
        let mut all: Vec<&HtmlItem<'_>> = layouts.iter().flat_map(|l| l.functions.iter()).collect();
        all.sort_by_cached_key(|item| (entry_sort_key(&item.entry.name), item.entry.name.clone()));
        let mut pages_html = function_page("mod-all", "", &all, false, true);
        for (m, layout) in modules.iter().zip(&layouts) {
            pages_html.push('\n');
            let intro = render_module_intro_html(m);
            let items: Vec<&HtmlItem<'_>> = layout.functions.iter().collect();
            pages_html.push_str(&function_page(&layout.page_id, &intro, &items, true, false));
        }
        pages_html
    } else {
        let intro = render_module_intro_html(&modules[0]);
        let items: Vec<&HtmlItem<'_>> = layouts[0].functions.iter().collect();
        function_page("mod-all", &intro, &items, true, true)
    };

    for (m, layout) in modules.iter().zip(&layouts) {
        if m.selectors.is_empty() {
            continue;
        }
        pages.push('\n');
        let items: Vec<&HtmlItem<'_>> = layout.selectors.iter().collect();
        let opts = PageOptions {
            id: &layout.selector_page_id,
            kind_plural: "selectors",
            intro: "",
            own_cards: true,
            active: false,
            is_selector: true,
        };
        pages.push_str(&build_entries_page(&opts, &items, &index));
    }
    // Last, so the script can leave it out of the module pages it routes between.
    pages.push('\n');
    pages.push_str(&build_search_page());

    let selector_section = if has_selectors {
        format!(
            "        <nav class=\"sidebar-section\">\n\
             \x20         <div class=\"sidebar-section-title\">Selectors</div>\n\
             {selector_sidebar_items}\
             \x20       </nav>\n"
        )
    } else {
        String::new()
    };

    let site_links = site_links_html();
    let style = include_str!("docs.css");
    let script = include_str!("docs.js");

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>mq - Function Reference</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Montserrat:wght@400;500;600;700&display=swap" rel="stylesheet" />
    <style>
{style}
    </style>
  </head>
  <body>
    <button class="sidebar-toggle" id="sidebarToggle" aria-label="Toggle navigation">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="18" x2="21" y2="18"/>
      </svg>
    </button>
    <div class="sidebar-overlay" id="sidebarOverlay"></div>

    <div class="layout">
      <aside class="sidebar" id="sidebar">
        <div class="sidebar-header">
          <h1>mq</h1>
          <p>Function Reference</p>
          <nav class="site-links" aria-label="mq sites">{site_links}</nav>
        </div>
        <div class="global-search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <input type="search" id="globalSearch" placeholder="Search all modules" aria-label="Search all modules" autocomplete="off" spellcheck="false" />
          <kbd title="Press / or Ctrl+K to search">/</kbd>
        </div>
        <nav class="sidebar-section">
          <div class="sidebar-section-title">Modules</div>
{sidebar_items}
        </nav>
{selector_section}
      </aside>

      <div class="content">
{pages}
      </div>
    </div>

    <footer>
      <p>
        Generated by <a href="https://github.com/harehare/mq">mq</a>
        <span class="footer-links">
          <a href="https://mqlang.org/llms.txt" target="_blank" rel="noopener">llms.txt</a>
        </span>
      </p>
    </footer>

    <script>
{script}
    </script>
  </body>
</html>"##,
    )
}

/// Generate an inline SVG icon with the given inner elements.
fn svg_icon(inner: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{inner}</svg>"
    )
}

/// Whether a module name is the native built-ins group.
fn is_builtin_name(name: &str) -> bool {
    name.starts_with("Built-in")
}

/// Return an appropriate SVG icon for a module name.
fn module_icon(name: &str) -> String {
    if is_builtin_name(name) {
        // cube icon
        svg_icon(
            "<path d=\"M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z\"/>\
             <polyline points=\"3.27 6.96 12 12.01 20.73 6.96\"/>\
             <line x1=\"12\" y1=\"22.08\" x2=\"12\" y2=\"12\"/>",
        )
    } else {
        // package icon
        svg_icon(
            "<line x1=\"16.5\" y1=\"9.4\" x2=\"7.5\" y2=\"4.21\"/>\
             <path d=\"M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z\"/>\
             <polyline points=\"3.27 6.96 12 12.01 20.73 6.96\"/>\
             <line x1=\"12\" y1=\"22.08\" x2=\"12\" y2=\"12\"/>",
        )
    }
}

/// Return an SVG icon for selector items.
fn selector_icon() -> String {
    // crosshair/target icon
    svg_icon(
        "<circle cx=\"12\" cy=\"12\" r=\"10\"/>\
         <line x1=\"22\" y1=\"12\" x2=\"18\" y2=\"12\"/>\
         <line x1=\"6\" y1=\"12\" x2=\"2\" y2=\"12\"/>\
         <line x1=\"12\" y1=\"6\" x2=\"12\" y2=\"2\"/>\
         <line x1=\"12\" y1=\"22\" x2=\"12\" y2=\"18\"/>",
    )
}

/// Escape HTML special characters.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: &'static str, aliases: &[&str]) -> DocEntry {
        DocEntry {
            name: name.to_string(),
            kind,
            params: vec![DocParam { name: "path".to_string(), type_name: "string".to_string() }],
            returns: "string".to_string(),
            description: String::new(),
            examples: Vec::new(),
            capability: None,
            related_module: None,
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn module(name: &str, functions: Vec<DocEntry>, selectors: Vec<DocEntry>) -> ModuleEntry {
        ModuleEntry { name: name.to_string(), description: String::new(), examples: Vec::new(), functions, selectors }
    }

    fn fixture() -> Vec<ModuleEntry> {
        vec![
            module(
                "Built-in",
                vec![entry("abs", "function", &[]), entry("path", "function", &[])],
                vec![entry(".text", "selector", &[".p"])],
            ),
            module("csv", vec![entry("csv_parse", "function", &[]), entry("abs", "function", &[])], Vec::new()),
        ]
    }

    #[test]
    fn permalinks_are_unique_and_module_scoped() {
        let modules = fixture();
        let layouts = layout_modules(&modules);

        assert_eq!(layouts[0].page_id, "mod-built-in");
        assert_eq!(layouts[1].page_id, "mod-csv");
        let anchors = |items: &[HtmlItem<'_>]| items.iter().map(|i| i.anchor.clone()).collect::<Vec<_>>();
        assert_eq!(anchors(&layouts[0].functions), ["abs", "path"]);
        assert_eq!(anchors(&layouts[0].selectors), [".text"]);
        assert_eq!(anchors(&layouts[1].functions), ["csv::abs", "csv::csv_parse"]);
    }

    #[test]
    fn href_fragment_encodes_selector_symbols() {
        assert_eq!(href_fragment("csv::csv_parse"), "#csv::csv_parse");
        assert_eq!(href_fragment(".[]"), "#.%5B%5D");
    }

    #[test]
    fn jump_letter_buckets_symbols() {
        assert_eq!(entry_letter("abs"), 'A');
        assert_eq!(entry_letter(".h1"), 'H');
        assert_eq!(entry_letter(".[]"), '*');
        assert_eq!(entry_letter("1st"), '*');
    }

    #[test]
    fn description_links_code_spans_but_not_params_or_self() {
        let modules = fixture();
        let layouts = layout_modules(&modules);
        let index = LinkIndex::new(&modules, &layouts);
        let params = [DocParam { name: "path".to_string(), type_name: "string".to_string() }];
        let ctx = LinkCtx { index: &index, module: 1, self_anchor: "csv::csv_parse", params: &params };

        let html = render_description_html("Like `abs` or `csv::abs` or `.p`, not `path` or `csv_parse(x)`.", &ctx);

        // Bare `abs` prefers the current module, `.p` resolves through its alias,
        // and the parameter name and the entry itself stay plain.
        assert!(html.contains("<a class=\"xref\" href=\"#csv::abs\"><code>abs</code></a>"));
        assert!(html.contains("<a class=\"xref\" href=\"#csv::abs\"><code>csv::abs</code></a>"));
        assert!(html.contains("<a class=\"xref\" href=\"#.text\"><code>.p</code></a>"));
        assert!(html.contains("<code>path</code>"));
        assert!(html.contains("<code>csv_parse(x)</code>"));
    }

    #[test]
    fn code_links_calls_only() {
        let modules = fixture();
        let layouts = layout_modules(&modules);
        let index = LinkIndex::new(&modules, &layouts);
        let ctx = LinkCtx { index: &index, module: 0, self_anchor: "path", params: &[] };

        let html =
            render_code_html("abs(1) | .abs | \"abs(2)\" | def abs(x): x; # abs(3)\ncsv::csv_parse(\"a\")", Some(&ctx));

        assert!(html.starts_with("<a class=\"xref\" href=\"#abs\"><span class=\"tok-fn\">abs</span></a>(<span class=\"tok-num\">1</span>)"));
        assert_eq!(html.matches("class=\"xref\"").count(), 2);
        assert!(html.contains("<a class=\"xref\" href=\"#csv::csv_parse\"><span class=\"tok-fn\">csv::csv_parse</span></a>"));
    }

    #[test]
    fn html_shows_aliases_and_renders_each_entry_once() {
        let html = format_html(&fixture());

        assert!(html.contains("Aliases: <code>.p</code>"));
        assert_eq!(html.matches("id=\".text\"").count(), 1);
        assert_eq!(html.matches("data-anchor=\".text\"").count(), 1);
        // The "All" page only lists `abs` by anchor; the script clones the one canonical card.
        assert_eq!(html.matches("data-anchor=\"abs\"").count(), 1);
        assert_eq!(html.matches("id=\"abs\"").count(), 1);
        assert!(html.contains("data-anchors=\"abs\""));
        assert!(html.contains("data-anchors=\"csv::abs csv::csv_parse\""));
        assert!(html.contains("href=\"#mod-csv\" data-module=\"mod-csv\""));
    }

    #[test]
    fn code_is_highlighted() {
        let html = render_code_html("let x = 1.5 | if (true): .h1 else: s\"a${x}\" # note", None);

        for expected in [
            "<span class=\"tok-kw\">let</span>",
            "<span class=\"tok-num\">1.5</span>",
            "<span class=\"tok-kw\">if</span>",
            "<span class=\"tok-const\">true</span>",
            "<span class=\"tok-sel\">.h1</span>",
            "<span class=\"tok-str\">s&quot;a${x}&quot;</span>",
            "<span class=\"tok-com\"># note</span>",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }
        // A bare identifier that is not called stays plain.
        assert!(html.contains(" x ") || html.contains("let</span> x"));
    }

    #[test]
    fn signature_omits_dynamic_types() {
        let mut e = entry("add", "function", &[]);
        e.params = vec![
            DocParam { name: "a".to_string(), type_name: "dynamic".to_string() },
            DocParam { name: "n".to_string(), type_name: "number".to_string() },
        ];
        e.returns = "dynamic".to_string();
        assert_eq!(signature_text(&e), "add(a, n: number)");

        e.returns = "number".to_string();
        assert_eq!(signature_text(&e), "add(a, n: number): number");
    }

    #[test]
    fn deprecation_notice_is_split_from_description() {
        let (body, note) = split_deprecation("Checks content. Deprecated: use has_body() instead.");
        assert_eq!(body, "Checks content.");
        assert_eq!(note.as_deref(), Some("Deprecated: use has_body() instead."));

        let (body, note) = split_deprecation("Old thing (deprecated). Still works.");
        assert_eq!(note.as_deref(), Some("Old thing (deprecated)."));
        assert_eq!(body, "Still works.");
    }

    #[test]
    fn only_standard_modules_link_to_their_source() {
        let csv = render_module_source_html("csv");
        assert!(csv.contains("/blob/v"));
        assert!(csv.contains("/crates/mq-lang/modules/csv.mq\""));
        assert!(!csv.contains("/edit/"));

        assert!(render_module_source_html("Built-in").is_empty());
        assert!(render_module_source_html("my/file.mq").is_empty());
    }

    #[test]
    fn html_groups_builtins_and_links_out() {
        let mut modules = fixture();
        modules[0].functions.push(entry("is_string", "function", &[]));
        let mut old = entry("old_fn", "function", &[]);
        old.description = "Does a thing. Deprecated: use abs() instead.".to_string();
        modules[1].functions.push(old);
        let html = format_html(&modules);

        // Built-ins sit under category headings, other modules under their own name.
        assert!(html.contains("<h2 class=\"group-title\">Math &amp; Statistics<span"));
        assert!(html.contains("<h2 class=\"group-title\">Types &amp; Logic<span"));
        assert!(html.contains("<h2 class=\"group-title\">csv<span"));
        assert!(html.contains("data-group=\"Types &amp; Logic\""));
        // Deprecated entries get a warning card and class; selectors stay A–Z.
        assert!(html.contains("<div class=\"entry-warning\" role=\"note\"><strong>Deprecated</strong>"));
        assert!(html.contains("class=\"entry entry-deprecated\""));
        assert!(html.contains("data-group=\"T\""));
        // Site links, the module's source link and llms.txt.
        for expected in [
            "https://mqlang.org/playground",
            "https://mqlang.org/book/cookbook",
            "https://mqlang.org/llms.txt",
            "/blob/v",
            "id=\"globalSearch\"",
        ] {
            assert!(html.contains(expected), "{expected} missing");
        }
        assert!(!html.contains("Edit on GitHub"));
    }
}
