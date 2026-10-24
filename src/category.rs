//! Groups the built-in functions under headings for the HTML reference. `mq-help` carries no
//! category, so names are mapped here; anything new falls back to a prefix rule or "Other".

use std::collections::HashMap;
use std::sync::LazyLock;

/// Display order of the categories and the built-in names filed under each.
const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Markdown",
        &[
            "attr", "decrease_header_depth", "demote_heading", "embed_images", "extract_images",
            "extract_urls", "from_html", "get_location", "get_title", "get_url",
            "increase_header_depth", "promote_heading", "sanitize_html", "set_attr", "set_check",
            "set_children", "set_code_block_lang", "set_list_ordered", "set_ref", "strip_tags",
            "to_blockquote", "to_break", "to_callout", "to_code", "to_code_inline", "to_definition",
            "to_delete", "to_em", "to_footnote", "to_footnote_ref", "to_h", "to_hr", "to_html",
            "to_image", "to_link", "to_markdown", "to_markdown_string", "to_math", "to_math_inline",
            "to_md_fragment", "to_md_html", "to_md_list", "to_md_name", "to_md_table_align",
            "to_md_table_cell", "to_md_table_row", "to_md_text", "to_mdx", "to_strong", "to_text",
        ],
    ),
    (
        "Markdown Checks",
        &[
            "is_callout", "is_code", "is_em", "is_h", "is_h1", "is_h2", "is_h3", "is_h4", "is_h5",
            "is_h6", "is_h_level", "is_html", "is_list", "is_mdx", "is_mdx_flow_expression",
            "is_mdx_js_esm", "is_mdx_jsx_flow_element", "is_mdx_jsx_text_element",
            "is_mdx_text_expression", "is_table_align", "is_table_cell", "is_text", "is_toml",
            "is_yaml",
        ],
    ),
    (
        "String",
        &[
            "ascii_downcase", "ascii_upcase", "casefold", "center", "chars", "chomp", "common_indent",
            "contains", "count_substr", "dedent", "downcase", "ends_with", "explode", "grapheme_len",
            "graphemes", "indent", "is_alnum", "is_alpha", "is_ascii", "is_digit", "is_lower",
            "is_space", "is_upper", "lines", "lpad", "ltrim", "ltrimstr", "normalize_newlines",
            "remove_chars", "repeat", "replace", "rpad", "rsplit_once", "rtrim", "rtrimstr", "slice",
            "slugify", "split", "split_once", "squeeze_whitespace", "starts_with", "strip_ansi",
            "token_count", "translate", "trim", "truncate", "unicode_normalize", "unlines", "upcase",
            "word_wrap", "words", "zfill",
        ],
    ),
    (
        "Array & Collection",
        &[
            "all", "any", "chunk_by", "chunks", "collect", "combinations", "compact", "compact_map",
            "difference", "each", "fill", "filter", "find", "find_index", "first", "flat_map",
            "flatten", "fold", "fold_while", "frequencies_by", "group_by", "implode", "index",
            "indices", "interleave", "interpose", "intersection", "join", "join_by", "last", "len",
            "map", "map_while", "ngram", "partition", "range", "reject", "reverse", "rindex", "second",
            "select", "skip", "skip_while", "sort", "sort_by", "sort_natural", "split_records", "take",
            "take_while", "tally", "token_compress", "transpose", "union", "uniq", "unique_by", "unzip",
            "zip",
        ],
    ),
    (
        "Dict & Path",
        &[
            "del", "del_path", "del_paths", "entries", "from_entries", "frontmatter", "get", "get_or",
            "get_path", "has", "index_by", "insert", "invert", "keys", "leaf_entries", "map_values",
            "merge_defaults", "merge_with", "omit", "paths", "pick", "pluck", "set", "set_path",
            "update", "update_path", "values", "walk", "with_entries",
        ],
    ),
    (
        "Math & Statistics",
        &[
            "abs", "accumulate", "acos", "add", "asin", "atan", "atan2", "band", "bnot", "bor",
            "bsearch", "cbrt", "ceil", "clamp", "cos", "cosh", "count_by", "cumavg", "cumsum",
            "describe", "div", "exp", "floor", "gcd", "geomean", "histogram", "human_bytes",
            "human_size", "hypot", "infinite", "is_integer", "is_nan", "lcm", "ln", "log", "log10",
            "log2", "max", "max_by", "mean", "median", "min", "min_by", "mod", "mode", "mul", "nan",
            "negate", "percentile", "pow", "product", "rand", "rand_int", "round", "shift_left",
            "shift_right", "sin", "sinh", "sqrt", "stats_by", "stddev", "sub", "sum", "sum_by", "tan",
            "tanh", "top_by", "trunc", "variance", "xor",
        ],
    ),
    (
        "Comparison & Logic",
        &[
            "and", "between", "coalesce", "complement", "eq", "gt", "gte", "identity", "in", "lt",
            "lte", "ne", "not", "or",
        ],
    ),
    (
        "Type Checks",
        &[
            "arrays", "booleans", "bytes", "dicts", "is_array", "is_blank", "is_bool", "is_bytes",
            "is_coroutine", "is_dict", "is_empty", "is_markdown", "is_none", "is_number", "is_string",
            "iterables", "markdowns", "nones", "numbers", "scalars", "strings", "type",
        ],
    ),
    (
        "Conversion",
        &[
            "array", "convert", "dict", "to_array", "to_boolean", "to_bytes", "to_coroutine",
            "to_number", "to_number_or", "to_string",
        ],
    ),
    (
        "Date & Time",
        &[
            "date_add", "date_diff", "date_relative", "from_date", "gmtime", "localtime", "mktime",
            "now", "strftime", "strptime", "to_date", "to_date_iso8601",
        ],
    ),
    (
        "Regex & Matching",
        &[
            "capture", "glob_match", "gsub", "is_not_regex_match", "is_regex_match", "redact",
            "regex_escape", "regex_match", "regex_replace", "replace_prose", "scan", "test",
        ],
    ),
    (
        "Encoding & Hashing",
        &[
            "base64", "base64d", "base64d_bytes", "base64url", "base64urld", "base64urld_bytes",
            "decode", "encode", "from_hex", "hexdump", "html_escape", "html_unescape",
            "markdown_escape", "md5", "pack", "sha256", "sha512", "shell_quote", "to_hex", "unpack",
            "url_decode", "url_encode", "utf8", "utf8bytelen",
        ],
    ),
    (
        "Paths & Files",
        &[
            "basename", "dirname", "extname", "path_join", "safe_filename", "stem",
        ],
    ),
    (
        "I/O & Network",
        &[
            "close", "collection", "env", "file_exists", "file_info", "file_size", "http", "http_all",
            "http_delete", "http_delete_json", "http_get", "http_get_json", "http_head", "http_lines",
            "http_patch", "http_patch_json", "http_post", "http_post_json", "http_put", "http_put_json",
            "input", "load_markdown", "next", "open_file", "open_http", "print", "read_bytes",
            "read_file", "read_file_bytes", "read_line", "send", "status", "stderr", "stream_bytes",
            "stream_chunks", "stream_lines", "stream_range", "system", "walk_files", "write_file",
        ],
    ),
    (
        "Random & IDs",
        &[
            "random_string", "sample", "shuffle", "uuid", "uuid_v4", "uuid_v7",
        ],
    ),
    (
        "Functional",
        &[
            "comp", "flip", "juxt", "partial", "tap",
        ],
    ),
    (
        "Debug & Errors",
        &[
            "all_symbols", "breakpoint", "debug", "error", "halt", "halt_error", "inspect", "intern",
        ],
    ),
];

static BY_NAME: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    CATEGORIES.iter().enumerate().flat_map(|(rank, (_, names))| names.iter().map(move |n| (*n, rank))).collect()
});

/// Rank (display order) and title of the category for a built-in function.
pub fn builtin_category(name: &str) -> (usize, &'static str) {
    let rank = BY_NAME.get(name).copied().or_else(|| fallback(name));
    rank.map_or((CATEGORIES.len(), "Other"), |rank| (rank, CATEGORIES[rank].0))
}

/// Number of built-in categories, including "Other" — module groups are ranked after these.
pub const fn builtin_category_count() -> usize {
    CATEGORIES.len() + 1
}

/// Prefix rules for names added to mq after this table was written.
fn fallback(name: &str) -> Option<usize> {
    let title = match name {
        _ if name.starts_with("is_") => "Type Checks",
        _ if name.starts_with("to_") => "Conversion",
        _ if name.starts_with("http_") || name.starts_with("read_") => "I/O & Network",
        _ => return None,
    };
    CATEGORIES.iter().position(|(t, _)| *t == title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_filed_once() {
        let total: usize = CATEGORIES.iter().map(|(_, names)| names.len()).sum();
        assert_eq!(total, BY_NAME.len());
    }

    #[test]
    fn unknown_names_use_prefix_rules_then_other() {
        assert_eq!(builtin_category("abs").1, "Math & Statistics");
        assert_eq!(builtin_category("is_something_new").1, "Type Checks");
        assert_eq!(builtin_category("frobnicate"), (CATEGORIES.len(), "Other"));
    }
}
