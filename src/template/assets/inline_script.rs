const TEMPLATE: &str = concat!(
    "(function() {\n",
    include_str!("js/bootstrap.js"),
    "\n",
    include_str!("js/selection.js"),
    "\n",
    include_str!("js/content-renderer.js"),
    "\n",
    include_str!("js/content-enhancements.js"),
    "\n",
    include_str!("js/content-navigation.js"),
    "\n",
    include_str!("js/document-search.js"),
    "\n",
    include_str!("js/directory-search.js"),
    "\n",
    include_str!("js/content-controller.js"),
    "\n",
    include_str!("js/memo.js"),
    "\n",
    include_str!("js/fetch.js"),
    "\n",
    include_str!("js/websocket.js"),
    "\n",
    include_str!("js/sidebar.js"),
    "\n",
    "startMarkdownViewApp();\n",
    "}());\n",
);

pub(super) fn inline_js(max_file_size: u64) -> String {
    TEMPLATE.replace("__MAX_FILE_SIZE_MB__", &file_size_display_mb(max_file_size))
}

fn file_size_display_mb(max_file_size: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    max_file_size.div_ceil(MIB).to_string()
}

#[cfg(test)]
mod tests {
    use super::{file_size_display_mb, inline_js, TEMPLATE};
    use tree_sitter::{Node, Parser};

    const MAX_FILE_SIZE_SENTINEL: &str = "__MAX_FILE_SIZE_MB__";

    fn count_occurrences(source: &str, needle: &str) -> usize {
        source.matches(needle).count()
    }

    fn max_file_size_mb_object_property_values(source: &str) -> Vec<Option<u64>> {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_javascript::language())
            .expect("JavaScript grammar should load");
        let tree = parser
            .parse(source, None)
            .expect("JavaScript source should parse");
        assert!(
            !tree.root_node().has_error(),
            "JavaScript source should not contain parse errors: {source}"
        );

        let mut values = Vec::new();
        collect_max_file_size_mb_object_property_values(tree.root_node(), source, &mut values);
        values
    }

    fn collect_max_file_size_mb_object_property_values(
        node: Node<'_>,
        source: &str,
        values: &mut Vec<Option<u64>>,
    ) {
        if node.kind() == "pair" {
            let key = node
                .child_by_field_name("key")
                .and_then(|key| static_object_property_key_name(key, source));
            if key.as_deref() == Some("maxFileSizeMb") {
                let value = node
                    .child_by_field_name("value")
                    .and_then(|value| numeric_literal_value(value, source));
                values.push(value);
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_max_file_size_mb_object_property_values(child, source, values);
        }
    }

    fn numeric_literal_value(node: Node<'_>, source: &str) -> Option<u64> {
        if node.kind() != "number" {
            return None;
        }
        node_text(node, source).parse().ok()
    }

    #[derive(Debug, Eq, PartialEq)]
    struct InnerHtmlSink {
        source: String,
    }

    fn inner_html_sinks(source: &str) -> Vec<InnerHtmlSink> {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_javascript::language())
            .expect("JavaScript grammar should load");
        let tree = parser
            .parse(source, None)
            .expect("JavaScript source should parse");
        assert!(
            !tree.root_node().has_error(),
            "JavaScript source should not contain parse errors: {source}"
        );

        let mut sinks = Vec::new();
        collect_inner_html_sinks(tree.root_node(), source, &mut sinks);
        sinks
    }

    fn collect_inner_html_sinks(node: Node<'_>, source: &str, sinks: &mut Vec<InnerHtmlSink>) {
        if matches!(
            node.kind(),
            "assignment_expression" | "augmented_assignment_expression"
        ) {
            if let Some(left) = node.child_by_field_name("left") {
                if contains_inner_html_member_access(left, source) {
                    sinks.push(InnerHtmlSink {
                        source: node_text(node, source).to_string(),
                    });
                }
            }
        }

        if node.kind() == "call_expression" && is_inner_html_mutating_call(node, source) {
            sinks.push(InnerHtmlSink {
                source: node_text(node, source).to_string(),
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_inner_html_sinks(child, source, sinks);
        }
    }

    fn contains_inner_html_member_access(node: Node<'_>, source: &str) -> bool {
        if matches!(node.kind(), "member_expression" | "subscript_expression")
            && member_property_name(node, source).as_deref() == Some("innerHTML")
        {
            return true;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if node.kind() == "subscript_expression"
                && node.child_by_field_name("index") == Some(child)
            {
                continue;
            }
            if contains_inner_html_member_access(child, source) {
                return true;
            }
        }
        false
    }

    fn is_inner_html_mutating_call(node: Node<'_>, source: &str) -> bool {
        let Some(function) = node.child_by_field_name("function") else {
            return false;
        };
        let Some(arguments) = node.child_by_field_name("arguments") else {
            return false;
        };

        match normalized_member_name(function, source).as_deref() {
            Some("Object.assign") => {
                let mut cursor = arguments.walk();
                for argument in arguments.named_children(&mut cursor).skip(1) {
                    if contains_top_level_inner_html_object_key(argument, source) {
                        return true;
                    }
                }
                false
            }
            Some("Object.defineProperties") => arguments.named_child(1).is_some_and(|descriptor| {
                contains_top_level_inner_html_object_key(descriptor, source)
            }),
            Some("Reflect.set" | "Object.defineProperty") => {
                arguments
                    .named_child(1)
                    .and_then(|property| static_computed_property_name(property, source))
                    .as_deref()
                    == Some("innerHTML")
            }
            _ => false,
        }
    }

    fn contains_top_level_inner_html_object_key(node: Node<'_>, source: &str) -> bool {
        let node = unwrap_parenthesized_expression(node);
        if node.kind() != "object" {
            return false;
        }

        let mut cursor = node.walk();
        for property in node.named_children(&mut cursor) {
            if object_property_key_name(property, source).as_deref() == Some("innerHTML") {
                return true;
            }
            if property.kind() == "spread_element"
                && property
                    .named_child(0)
                    .is_some_and(|spread| contains_top_level_inner_html_object_key(spread, source))
            {
                return true;
            }
        }
        false
    }

    fn unwrap_parenthesized_expression(mut node: Node<'_>) -> Node<'_> {
        while node.kind() == "parenthesized_expression" {
            let Some(inner) = node.named_child(0) else {
                break;
            };
            node = inner;
        }
        node
    }

    fn unwrap_static_callee_expression(mut node: Node<'_>) -> Node<'_> {
        loop {
            node = unwrap_parenthesized_expression(node);
            if node.kind() != "sequence_expression" {
                return node;
            }

            let Some(last) = last_named_child(node) else {
                return node;
            };
            node = last;
        }
    }

    fn last_named_child(node: Node<'_>) -> Option<Node<'_>> {
        let mut cursor = node.walk();
        let mut last = None;
        for child in node.named_children(&mut cursor) {
            last = Some(child);
        }
        last
    }

    fn object_property_key_name(node: Node<'_>, source: &str) -> Option<String> {
        if node.kind() == "shorthand_property_identifier" {
            return static_identifier_like_property_name(node, source);
        }

        if node.kind() == "pair" {
            return node
                .child_by_field_name("key")
                .and_then(|key| static_object_property_key_name(key, source));
        }

        if node.kind() == "method_definition" {
            return node
                .child_by_field_name("name")
                .and_then(|name| static_object_property_key_name(name, source));
        }

        None
    }

    fn normalized_member_name(node: Node<'_>, source: &str) -> Option<String> {
        let node = unwrap_static_callee_expression(node);
        let object = node.child_by_field_name("object")?;
        let object = static_identifier_name(object, source)?;
        let property = member_property_name(node, source)?;
        Some(format!("{object}.{property}"))
    }

    fn member_property_name(node: Node<'_>, source: &str) -> Option<String> {
        if let Some(property) = node.child_by_field_name("property") {
            return static_identifier_like_property_name(property, source);
        }

        if let Some(index) = node.child_by_field_name("index") {
            return static_computed_property_name(index, source);
        }

        let mut cursor = node.walk();
        let mut property = None;
        for child in node.children(&mut cursor).filter(|child| child.is_named()) {
            if let Some(name) = static_identifier_like_property_name(child, source) {
                property = Some(name);
            }
        }
        property
    }

    fn static_identifier_like_property_name(node: Node<'_>, source: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "property_identifier" | "shorthand_property_identifier" => {
                decode_js_identifier(node_text(node, source))
            }
            "string" | "template_string" => decode_js_static_string(node_text(node, source)),
            _ => None,
        }
    }

    fn static_computed_property_name(node: Node<'_>, source: &str) -> Option<String> {
        let node = unwrap_parenthesized_expression(node);
        match node.kind() {
            "computed_property_name" => node
                .named_child(0)
                .and_then(|property| static_computed_property_name(property, source)),
            "string" | "template_string" => decode_js_static_string(node_text(node, source)),
            _ => None,
        }
    }

    fn static_object_property_key_name(node: Node<'_>, source: &str) -> Option<String> {
        if node.kind() == "computed_property_name" {
            return static_computed_property_name(node, source);
        }
        static_identifier_like_property_name(node, source)
    }

    fn static_identifier_name(node: Node<'_>, source: &str) -> Option<String> {
        let node = unwrap_parenthesized_expression(node);
        if node.kind() == "identifier" {
            return decode_js_identifier(node_text(node, source));
        }
        None
    }

    fn decode_js_identifier(raw: &str) -> Option<String> {
        let mut decoded = String::new();
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                decoded.push(ch);
                continue;
            }

            if chars.next()? != 'u' {
                return None;
            }
            decoded.push(decode_unicode_escape(&mut chars)?);
        }
        Some(decoded)
    }

    fn decode_js_static_string(raw: &str) -> Option<String> {
        let quote = raw.chars().next()?;
        if !matches!(quote, '\'' | '"' | '`') || raw.chars().last()? != quote {
            return None;
        }
        if quote == '`' && raw.contains("${") {
            return None;
        }

        let inner = &raw[quote.len_utf8()..raw.len() - quote.len_utf8()];
        let mut decoded = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                decoded.push(ch);
                continue;
            }

            let escaped = chars.next()?;
            match escaped {
                'x' => decoded.push(char_from_hex(&mut chars, 2)?),
                'u' => decoded.push(decode_unicode_escape(&mut chars)?),
                'n' => decoded.push('\n'),
                'r' => decoded.push('\r'),
                't' => decoded.push('\t'),
                'b' => decoded.push('\u{0008}'),
                'f' => decoded.push('\u{000C}'),
                'v' => decoded.push('\u{000B}'),
                '0'..='7' => decoded.push(decode_legacy_octal_escape(escaped, &mut chars)?),
                '\n' => {}
                '\r' => {
                    if matches!(chars.clone().next(), Some('\n')) {
                        chars.next();
                    }
                }
                other => decoded.push(other),
            }
        }

        Some(decoded)
    }

    fn decode_unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
        if matches!(chars.clone().next(), Some('{')) {
            chars.next();
            let mut hex = String::new();
            for hex_ch in chars.by_ref() {
                if hex_ch == '}' {
                    return char::from_u32(u32::from_str_radix(&hex, 16).ok()?);
                }
                hex.push(hex_ch);
            }
            return None;
        }

        char_from_hex(chars, 4)
    }

    fn decode_legacy_octal_escape(first: char, chars: &mut std::str::Chars<'_>) -> Option<char> {
        let mut octal = String::from(first);
        for _ in 0..2 {
            let Some(next) = chars.clone().next() else {
                break;
            };
            if !matches!(next, '0'..='7') {
                break;
            }
            chars.next();
            octal.push(next);
        }

        char::from_u32(u32::from_str_radix(&octal, 8).ok()?)
    }

    fn char_from_hex(chars: &mut std::str::Chars<'_>, len: usize) -> Option<char> {
        let mut hex = String::new();
        for _ in 0..len {
            hex.push(chars.next()?);
        }
        char::from_u32(u32::from_str_radix(&hex, 16).ok()?)
    }

    fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
        node.utf8_text(source.as_bytes())
            .expect("node text should be valid UTF-8")
    }

    #[test]
    fn innerhtml_scannerは壊れたjsを失敗させる() {
        let result = std::panic::catch_unwind(|| inner_html_sinks("target.innerHTML = ;"));

        assert!(result.is_err());
    }

    #[test]
    fn innerhtml_scannerはunicode_escape付きidentifierを検出する() {
        assert_eq!(
            inner_html_sinks(r#"target.\u0069nnerHTML = unsafeHtml;"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはaugmented_assignmentを検出する() {
        assert_eq!(
            inner_html_sinks(r#"target.innerHTML += unsafeHtml;"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはcomputed_object_keyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ["innerHTML"]: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ["inner\x48TML"]: unsafeHtml });"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_object_keyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, ({ innerHTML: unsafeHtml }));"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, (({ innerHTML: unsafeHtml })));"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperties(target, ({ innerHTML: { value: unsafeHtml } }));"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはstatic_object_spread内のinnerhtml_keyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ...{ innerHTML: unsafeHtml } });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ...({ ["innerHTML"]: unsafeHtml }) });"#)
                .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ...{ ...{ innerHTML: unsafeHtml } } });"#)
                .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperties(target, { ...{ innerHTML: { value: unsafeHtml } } });"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはdynamic_computed_object_keyをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { [innerHTML]: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperties(target, { [innerHTML]: { value: unsafeHtml } });"#
            )
            .len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_object内の非top_levelとdynamic_keyをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, ({ options: { innerHTML: unsafeHtml } }));"#)
                .len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, ({ [innerHTML]: unsafeHtml }));"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはdynamic_object_spreadをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ...source });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { ...makeObject() });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.assign(target, { ...{ options: { innerHTML: unsafeHtml } } });"#
            )
            .len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign({ ...{ innerHTML: safeDefault } }, source);"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはshorthand_object_propertyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { innerHTML });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.defineProperties(target, { innerHTML });"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはmethod_style_object_propertyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { innerHTML() { return unsafeHtml; } });"#)
                .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.assign(target, { get innerHTML() { return unsafeHtml; } });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.assign(target, { ["inner\x48TML"]() { return unsafeHtml; } });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperties(target, { innerHTML() { return unsafeHtml; } });"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerは危険apiのnested_object_keyをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { options: { innerHTML: unsafeHtml } });"#)
                .len(),
            0
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperties(target, { options: { innerHTML: { value: unsafeHtml } } });"#
            )
            .len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign({ innerHTML: safeDefault }, source);"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはdefine_propertiesを検出する() {
        assert_eq!(
            inner_html_sinks(
                r#"Object["defineProperties"](target, { innerHTML: { value: unsafeHtml } });"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはlegacy_octal_escapeを検出する() {
        assert_eq!(
            inner_html_sinks(r#"target["inner\110TML"] = unsafeHtml;"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerは文字列escapeされたcomputed_propertyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"target["inner\x48TML"] = unsafeHtml;"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"target["inner\u0048TML"] = unsafeHtml;"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"target[`inner\u0048TML`] = unsafeHtml;"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_computed_propertyを検出する() {
        assert_eq!(
            inner_html_sinks(r#"target[("innerHTML")] = unsafeHtml;"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"target[(("innerHTML"))] = unsafeHtml;"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Reflect.set(target, ("innerHTML"), unsafeHtml);"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object.defineProperty(target, ("innerHTML"), { value: unsafeHtml });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { [("innerHTML")]: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { [("inner\x48TML")]: unsafeHtml });"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはdynamic_computed_memberをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"target[innerHTML] = unsafeHtml;"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"target[other.innerHTML] = unsafeHtml;"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_dynamic_computed_propertyをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"target[(innerHTML)] = unsafeHtml;"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.assign(target, { [(innerHTML)]: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Reflect.set(target, (innerHTML), unsafeHtml);"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはcomputed_memberの危険api呼び出しを検出する() {
        assert_eq!(
            inner_html_sinks(r#"Object["assign"](target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"Reflect["set"](target, "innerHTML", unsafeHtml);"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"Object["defineProperty"](target, "innerHTML", { value: unsafeHtml });"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_mutating_calleeを検出する() {
        assert_eq!(
            inner_html_sinks(r#"(Object.assign)(target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"((Object["assign"]))(target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"(Reflect.set)(target, "innerHTML", unsafeHtml);"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(Object.defineProperty)(target, "innerHTML", { value: unsafeHtml });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(Object.defineProperties)(target, { innerHTML: { value: unsafeHtml } });"#
            )
            .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはsequence_wrapped_mutating_calleeを検出する() {
        assert_eq!(
            inner_html_sinks(r#"(0, Object.assign)(target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"((0, Object["assign"]))(target, { innerHTML: unsafeHtml });"#)
                .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"(0, Reflect.set)(target, "innerHTML", unsafeHtml);"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(0, Object.defineProperty)(target, "innerHTML", { value: unsafeHtml });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(0, Object["defineProperties"])(target, { innerHTML: { value: unsafeHtml } });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"(ignored, (Object).assign)(target, { innerHTML: unsafeHtml });"#)
                .len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはparenthesized_mutator_receiverを検出する() {
        assert_eq!(
            inner_html_sinks(r#"(Object).assign(target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"(Object)["assign"](target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(Object).defineProperty(target, "innerHTML", { value: unsafeHtml });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"(Object)["defineProperties"](target, { innerHTML: { value: unsafeHtml } });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"(Reflect).set(target, "innerHTML", unsafeHtml);"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはdynamic_computed_api_propertyをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"Object[assign](target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Reflect.set(target, innerHTML, unsafeHtml);"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"Object.defineProperty(target, innerHTML, { value: unsafeHtml });"#)
                .len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"(Object[assign])(target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"(Object)[assign](target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"(receiver).assign(target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはdynamic_sequence_wrapped_mutating_calleeをsink扱いしない() {
        assert_eq!(
            inner_html_sinks(r#"(0, Object[assign])(target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"(0, receiver.assign)(target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(r#"(Object.assign, other)(target, { innerHTML: unsafeHtml });"#).len(),
            0
        );
        assert_eq!(
            inner_html_sinks(
                r#"(Object.assign, Object[assign])(target, { innerHTML: unsafeHtml });"#
            )
            .len(),
            0
        );
    }

    #[test]
    fn innerhtml_scannerはunicode_escape付きcallee_objectの危険api呼び出しを検出する() {
        assert_eq!(
            inner_html_sinks(r#"\u004fbject.assign(target, { innerHTML: unsafeHtml });"#).len(),
            1
        );
        assert_eq!(
            inner_html_sinks(
                r#"\u004fbject["defineProperties"](target, { innerHTML: { value: unsafeHtml } });"#
            )
            .len(),
            1
        );
        assert_eq!(
            inner_html_sinks(r#"\u0052eflect.set(target, "innerHTML", unsafeHtml);"#).len(),
            1
        );
    }

    #[test]
    fn innerhtml_scannerはdestructuring_assignment内のsinkを検出する() {
        assert_eq!(
            inner_html_sinks(r#"({ html: target.innerHTML } = payload);"#).len(),
            1
        );
    }

    #[test]
    fn test_max_file_size_sentinelはbootstrap_jsだけに存在する() {
        let allowed_bootstrap_js = include_str!("js/bootstrap.js");
        let expected_sentinel_count =
            count_occurrences(allowed_bootstrap_js, MAX_FILE_SIZE_SENTINEL);
        // 違反時にファイル名を出すため、bootstrap.js以外のTEMPLATE includeと同期する。
        let disallowed_sources = [
            ("js/selection.js", include_str!("js/selection.js")),
            (
                "js/content-renderer.js",
                include_str!("js/content-renderer.js"),
            ),
            (
                "js/content-enhancements.js",
                include_str!("js/content-enhancements.js"),
            ),
            (
                "js/content-navigation.js",
                include_str!("js/content-navigation.js"),
            ),
            (
                "js/document-search.js",
                include_str!("js/document-search.js"),
            ),
            (
                "js/directory-search.js",
                include_str!("js/directory-search.js"),
            ),
            (
                "js/content-controller.js",
                include_str!("js/content-controller.js"),
            ),
            ("js/memo.js", include_str!("js/memo.js")),
            ("js/fetch.js", include_str!("js/fetch.js")),
            ("js/websocket.js", include_str!("js/websocket.js")),
            ("js/sidebar.js", include_str!("js/sidebar.js")),
        ];

        assert_eq!(
            expected_sentinel_count, 1,
            "bootstrap.js の max file size sentinel 出現回数が変わった"
        );

        let mut listed_sentinel_count = expected_sentinel_count;
        for (path, source) in disallowed_sources {
            let source_sentinel_count = count_occurrences(source, MAX_FILE_SIZE_SENTINEL);
            listed_sentinel_count += source_sentinel_count;
            assert!(
                source_sentinel_count == 0,
                "{path} に max file size sentinel が混入している"
            );
        }

        assert_eq!(
            count_occurrences(TEMPLATE, MAX_FILE_SIZE_SENTINEL),
            listed_sentinel_count,
            "JS template include一覧とsentinel契約テストの一覧が同期していない"
        );
    }

    #[test]
    fn test_inline_jsはmax_file_size由来のmb値へ置換する() {
        let generated = inline_js(crate::server::MAX_FILE_SIZE);

        assert!(
            !generated.contains(MAX_FILE_SIZE_SENTINEL),
            "生成済み JS に max file size sentinel が残っている"
        );
        assert_eq!(
            max_file_size_mb_object_property_values(&generated),
            vec![Some(
                file_size_display_mb(crate::server::MAX_FILE_SIZE)
                    .parse::<u64>()
                    .expect("MAX_FILE_SIZE display value should be numeric")
            )],
            "生成済み JS の object property maxFileSizeMb が MAX_FILE_SIZE 由来のMB表示1件になっていない"
        );
    }

    #[test]
    fn test_inline_jsは非整数mibのmax_file_sizeを切り上げ表示する() {
        let generated = inline_js(11_000_000);

        assert_eq!(
            max_file_size_mb_object_property_values(&generated),
            vec![Some(11)],
            "非整数MiBの object property maxFileSizeMb は過小表示を避けるため切り上げた値1件にする"
        );
    }

    #[test]
    fn inline_jsのinnerhtml_sinkは許可済み境界だけに限定する() {
        let sinks = inner_html_sinks(&super::inline_js(10 * 1024 * 1024));
        let sources: Vec<_> = sinks.iter().map(|sink| sink.source.as_str()).collect();

        assert_eq!(
            sources,
            vec![
                "contentEl.innerHTML = content",
                "tocEl.innerHTML = toc",
                "ctx.elements.documentSearchResultsEl.innerHTML = ''",
                "ctx.elements.documentSearchResultsEl.innerHTML = ''",
                "appContext.elements.memoPreviewEl.innerHTML = data.html",
                "appContext.elements.memoPreviewEl.innerHTML = ''",
            ]
        );
    }
}
