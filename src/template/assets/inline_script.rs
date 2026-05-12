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
    TEMPLATE.replace(
        "__MAX_FILE_SIZE_MB__",
        &(max_file_size / 1024 / 1024).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use tree_sitter::{Node, Parser};

    #[derive(Debug, Eq, PartialEq)]
    struct InnerHtmlSink {
        source: String,
    }

    fn inner_html_sinks(source: &str) -> Vec<InnerHtmlSink> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
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
                    .and_then(|property| static_property_name(property, source))
                    .as_deref()
                    == Some("innerHTML")
            }
            _ => false,
        }
    }

    fn contains_top_level_inner_html_object_key(node: Node<'_>, source: &str) -> bool {
        if node.kind() != "object" {
            return false;
        }

        let mut cursor = node.walk();
        for property in node.named_children(&mut cursor) {
            if object_property_key_name(property, source).as_deref() == Some("innerHTML") {
                return true;
            }
        }
        false
    }

    fn object_property_key_name(node: Node<'_>, source: &str) -> Option<String> {
        if node.kind() == "shorthand_property_identifier" {
            return static_property_name(node, source);
        }

        if node.kind() == "pair" {
            return node
                .child_by_field_name("key")
                .and_then(|key| static_property_name(key, source));
        }

        if node.kind() == "method_definition" {
            return node
                .child_by_field_name("name")
                .and_then(|name| static_property_name(name, source));
        }

        None
    }

    fn normalized_member_name(node: Node<'_>, source: &str) -> Option<String> {
        let object = node.child_by_field_name("object")?;
        let object = static_identifier_name(object, source)?;
        let property = member_property_name(node, source)?;
        Some(format!("{object}.{property}"))
    }

    fn member_property_name(node: Node<'_>, source: &str) -> Option<String> {
        if let Some(property) = node.child_by_field_name("property") {
            return static_property_name(property, source);
        }

        if let Some(index) = node.child_by_field_name("index") {
            return static_property_name(index, source);
        }

        let mut cursor = node.walk();
        let mut property = None;
        for child in node.children(&mut cursor).filter(|child| child.is_named()) {
            if let Some(name) = static_property_name(child, source) {
                property = Some(name);
            }
        }
        property
    }

    fn static_property_name(node: Node<'_>, source: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "property_identifier" | "shorthand_property_identifier" => {
                decode_js_identifier(node_text(node, source))
            }
            "computed_property_name" => node
                .named_child(0)
                .and_then(|property| static_property_name(property, source)),
            "string" | "template_string" => decode_js_static_string(node_text(node, source)),
            _ => None,
        }
    }

    fn static_identifier_name(node: Node<'_>, source: &str) -> Option<String> {
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
