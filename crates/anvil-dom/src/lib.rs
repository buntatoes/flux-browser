//! Document tree. Nodes live in one list and point at each other by id.
//!
//! First stage of the pipeline. [`parse_html`] turns source into a [`Document`]
//! this crate owns. anvil-style reads the tree next and does not mutate it.
//! anvil-layout and anvil-paint never see the HTML source. anvil-script mutates
//! the same document. anvil-browser is the owner of the live page.
//!
//! `<img src>` and `<video src>` (or the first `<source src>`) are recorded
//! as element attributes. Pixel decode is anvil-image's job. Timed video is
//! anvil-video's job. This crate does not decode either.
//!
//! Hard caps, enforced by the parser and by [`Document::set_inner_html`]:
//! [`MAX_DOM_DEPTH`] is 256 open elements, [`MAX_DOM_NODES`] is 50_000 nodes,
//! and [`MAX_ATTRS_PER_ELEMENT`] is 128 attributes on one start tag.
//! Nodes detached by edits stay in the list until the document is dropped.
//!
//! Selectors here are a single compound: a tag, `#id`, `.class`, `tag#id`,
//! or `tag.class`. Descendant combinators and the cascade live in anvil-style.

mod html;

pub use html::{MAX_ATTRS_PER_ELEMENT, MAX_DOM_DEPTH, MAX_DOM_NODES, parse_html};

/// Index of a node inside one [`Document`]'s node list.
///
/// The id is a copy of the slot index. It does not own the node. An id from
/// another document, or past [`Document::node_count`], is not valid here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Build an id from a raw slot index. Does not check that a node exists.
    pub fn from_raw(index: u32) -> Self {
        Self(index)
    }

    /// Slot index. Used to index the document's node vector.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Why [`Document::append`] refused an edit. The tree is left unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeError {
    /// `parent` or `child` is not a slot in this document.
    Missing,
    /// `child` is `parent`, or `parent` is already inside `child`.
    Cycle,
}

/// Payload owned by one node. The tree shape lives on [`Node`], not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeData {
    /// The document slot at index 0. It has no tag and no text.
    Document,
    /// An element. `tag` is ASCII-lowercase and owned. `attrs` owns each
    /// name and value. Names are stored lowercase by the writers in this crate.
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
    },
    /// Owned character data. Not parsed as markup.
    Text(String),
}

impl NodeData {
    /// An element with an owned lowercase tag and no attributes.
    /// `tag` is borrowed only for the copy.
    pub fn element(tag: &str) -> Self {
        Self::Element {
            tag: tag.to_ascii_lowercase(),
            attrs: Vec::new(),
        }
    }

    /// The element tag, borrowed from this node. `None` for the document and for text.
    pub fn tag(&self) -> Option<&str> {
        match self {
            Self::Element { tag, .. } => Some(tag),
            _ => None,
        }
    }

    /// The text, borrowed from this node. `None` unless this is [`NodeData::Text`].
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Attribute value borrowed from this element. `None` if this is not an
    /// element or the name is absent. Lookup ignores ASCII case.
    pub fn attr(&self, name: &str) -> Option<&str> {
        let Self::Element { attrs, .. } = self else {
            return None;
        };
        attrs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The `id` attribute, borrowed. `None` when it is missing.
    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }

    /// True when `class` contains `class_name` as a whole whitespace-separated token.
    /// A missing class attribute is false. The name is not trimmed here.
    pub fn has_class(&self, class_name: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| classes.split_ascii_whitespace().any(|c| c == class_name))
    }

    /// Set an attribute, storing the name in lowercase. No-op for non-elements.
    /// Replaces an existing name. `value` is copied.
    pub fn set_attr(&mut self, name: &str, value: &str) {
        let Self::Element { attrs, .. } = self else {
            return;
        };
        let key = name.to_ascii_lowercase();
        if let Some((_, slot)) = attrs.iter_mut().find(|(k, _)| *k == key) {
            *slot = value.to_string();
        } else {
            attrs.push((key, value.to_string()));
        }
    }

    /// Remove every attribute whose name matches, ignoring ASCII case.
    /// Returns false for a non-element or when nothing was removed.
    pub fn remove_attr(&mut self, name: &str) -> bool {
        let Self::Element { attrs, .. } = self else {
            return false;
        };
        let before = attrs.len();
        attrs.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        attrs.len() != before
    }

    /// Append a class token if it is non-empty and not already present.
    /// No-op for non-elements. The class string is owned by the attribute.
    pub fn add_class(&mut self, class_name: &str) {
        let class_name = class_name.trim();
        if class_name.is_empty() || self.has_class(class_name) {
            return;
        }
        let next = match self.attr("class") {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{} {class_name}", existing.trim())
            }
            _ => class_name.to_string(),
        };
        self.set_attr("class", &next);
    }

    /// Drop one class token. A missing class attribute is a no-op.
    /// The remaining tokens stay in one owned `class` value, which may be empty.
    pub fn remove_class(&mut self, class_name: &str) {
        let class_name = class_name.trim();
        let Some(existing) = self.attr("class") else {
            return;
        };
        let kept: Vec<&str> = existing
            .split_ascii_whitespace()
            .filter(|c| *c != class_name)
            .collect();
        let joined = kept.join(" ");
        self.set_attr("class", &joined);
    }

    /// Add the class and return true, or remove it and return false.
    /// The bool is the class's presence after the call.
    pub fn toggle_class(&mut self, class_name: &str) -> bool {
        if self.has_class(class_name) {
            self.remove_class(class_name);
            false
        } else {
            self.add_class(class_name);
            true
        }
    }
}

/// One slot in a [`Document`]. The document owns the node. Children are ids,
/// not nested `Node` values. `parent` is `None` for the root and for a node
/// that has been detached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    id: NodeId,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    data: NodeData,
}

impl Node {
    /// This slot's id. It is a copy, not a borrow of the document.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Parent id. `None` if this is the root or the node was detached.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Borrowed payload. The node keeps ownership.
    pub fn data(&self) -> &NodeData {
        &self.data
    }

    /// Mutable payload. Tree links are not exposed here.
    pub fn data_mut(&mut self) -> &mut NodeData {
        &mut self.data
    }
}

/// A document and every node it owns.
///
/// Slots are append-only. [`Self::remove_node`], [`Self::set_text_content`],
/// and [`Self::set_inner_html`] unlink children but leave the old slots in
/// `nodes`, so [`Self::node_count`] does not shrink. The skeleton is the
/// document node, then `html`, `head`, and `body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    nodes: Vec<Node>,
    html: NodeId,
    head: NodeId,
    body: NodeId,
}

impl Default for Document {
    /// Same document as [`Document::new`]: document, `html`, `head`, and `body`.
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty document with `html`, `head`, and `body` already in place.
    ///
    /// The returned value owns four nodes: document, `html`, `head`, `body`.
    pub fn new() -> Self {
        let mut doc = Self {
            nodes: Vec::new(),
            html: NodeId(0),
            head: NodeId(0),
            body: NodeId(0),
        };
        let root = doc.alloc(NodeData::Document);
        let html = doc.alloc(NodeData::element("html"));
        let head = doc.alloc(NodeData::element("head"));
        let body = doc.alloc(NodeData::element("body"));
        doc.append(root, html).expect("document skeleton");
        doc.append(html, head).expect("document skeleton");
        doc.append(html, body).expect("document skeleton");
        doc.html = html;
        doc.head = head;
        doc.body = body;
        doc
    }

    /// The document node. Always slot 0. Not the `html` element.
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// The `html` element created by [`Self::new`].
    pub fn html(&self) -> NodeId {
        self.html
    }

    /// The `head` element created by [`Self::new`].
    pub fn head(&self) -> NodeId {
        self.head
    }

    /// The `body` element created by [`Self::new`].
    pub fn body(&self) -> NodeId {
        self.body
    }

    /// Slots allocated, including detached nodes that are no longer in the tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Borrow the node at `id`. Panics if `id` is outside this document.
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Mutably borrow the node. `None` if `id` is outside this document.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.index())
    }

    /// Child ids in tree order, borrowed from the parent. Panics if `id` is outside.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id.index()].children
    }

    /// Allocate an element that is not yet in the tree (`parent` is `None`).
    ///
    /// `tag` is copied and lowercased. `attrs` is consumed; each name is lowercased.
    /// The returned id is owned by this document.
    pub fn create_element(&mut self, tag: &str, attrs: Vec<(String, String)>) -> NodeId {
        let mut data = NodeData::element(tag);
        if let NodeData::Element { attrs: slot, .. } = &mut data {
            *slot = attrs
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value))
                .collect();
        }
        self.alloc(data)
    }

    /// Allocate a text node that is not yet in the tree. `text` is copied.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.alloc(NodeData::Text(text.to_string()))
    }

    /// Set one attribute. No-op when `id` is outside the document or not an element.
    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: &str) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.data.set_attr(name, value);
        }
    }

    /// Remove one attribute. False when `id` is missing or the attribute was absent.
    pub fn remove_attribute(&mut self, id: NodeId, name: &str) -> bool {
        self.nodes
            .get_mut(id.index())
            .is_some_and(|node| node.data.remove_attr(name))
    }

    /// Add a class token on `id`. No-op when `id` is missing or not an element.
    pub fn add_class(&mut self, id: NodeId, class_name: &str) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.data.add_class(class_name);
        }
    }

    /// Remove a class token on `id`. No-op when `id` is missing.
    pub fn remove_class(&mut self, id: NodeId, class_name: &str) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.data.remove_class(class_name);
        }
    }

    /// Toggle a class on `id`. False when `id` is missing, or when the class
    /// was removed. True when the class is present after the call.
    pub fn toggle_class(&mut self, id: NodeId, class_name: &str) -> bool {
        self.nodes
            .get_mut(id.index())
            .is_some_and(|node| node.data.toggle_class(class_name))
    }

    /// Owned current value of a form control.
    ///
    /// A missing node yields an empty string. A `select` uses [`Self::select_value`].
    /// Any element with a `value` attribute returns that attribute, including a
    /// `textarea`. A `textarea` without `value` returns its text content.
    /// Other nodes without `value` yield an empty string.
    pub fn input_value(&self, id: NodeId) -> String {
        let Some(node) = self.nodes.get(id.index()) else {
            return String::new();
        };
        let data = node.data();
        if data.tag() == Some("select") {
            return self.select_value(id);
        }
        if let Some(val) = data.attr("value") {
            return val.to_string();
        }
        if data.tag() == Some("textarea") {
            return self.text_content(id);
        }
        String::new()
    }

    /// Store `value` on the `value` attribute. For a `textarea`, also replace
    /// its text content with the same string. No-op when `id` is missing.
    /// `value` is copied.
    pub fn set_input_value(&mut self, id: NodeId, value: &str) {
        let tag = self
            .nodes
            .get(id.index())
            .and_then(|n| n.data().tag().map(str::to_string));
        self.set_attribute(id, "value", value);
        if tag.as_deref() == Some("textarea") {
            self.set_text_content(id, value);
        }
    }

    /// True when a `checked` attribute is present and is not the word `false`.
    /// A missing node is false.
    pub fn is_checked(&self, id: NodeId) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        node.data()
            .attr("checked")
            .is_some_and(|val| !val.eq_ignore_ascii_case("false"))
    }

    /// Set `checked="checked"`, or remove `checked` when `checked` is false.
    pub fn set_checked(&mut self, id: NodeId, checked: bool) {
        if checked {
            self.set_attribute(id, "checked", "checked");
        } else {
            self.remove_attribute(id, "checked");
        }
    }

    /// Flip [`Self::is_checked`] and return the new state.
    pub fn toggle_checked(&mut self, id: NodeId) -> bool {
        let next = !self.is_checked(id);
        self.set_checked(id, next);
        next
    }

    /// True for `textarea`, or for `input` whose type is empty, `text`,
    /// `search`, `password`, `email`, `url`, `tel`, or `number`.
    /// A missing node is false. A missing `type` counts as text.
    pub fn is_text_input(&self, id: NodeId) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        let data = node.data();
        match data.tag() {
            Some("textarea") => true,
            Some("input") => {
                let kind = data.attr("type").unwrap_or("text").trim();
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "" | "text" | "search" | "password" | "email" | "url" | "tel" | "number"
                )
            }
            _ => false,
        }
    }

    /// True only for `<input type="checkbox">`. A missing node is false.
    pub fn is_checkbox_input(&self, id: NodeId) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        let data = node.data();
        data.tag() == Some("input")
            && data
                .attr("type")
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("checkbox"))
    }

    /// True when `id` is a `select` element. A missing node is false.
    pub fn is_select_control(&self, id: NodeId) -> bool {
        self.nodes
            .get(id.index())
            .is_some_and(|n| n.data().tag() == Some("select"))
    }

    /// Direct `option` children, in tree order. An owned list of ids.
    /// A missing node yields an empty vec. Nested options are not walked.
    pub fn select_options(&self, select_id: NodeId) -> Vec<NodeId> {
        let Some(node) = self.nodes.get(select_id.index()) else {
            return Vec::new();
        };
        node.children
            .iter()
            .copied()
            .filter(|cid| self.get(*cid).data().tag() == Some("option"))
            .collect()
    }

    /// The first option with `selected` set and not equal to `false`.
    /// If none is marked, the first option. `None` when there are no options.
    pub fn selected_option(&self, select_id: NodeId) -> Option<NodeId> {
        let opts = self.select_options(select_id);
        opts.iter()
            .copied()
            .find(|oid| {
                self.get(*oid)
                    .data()
                    .attr("selected")
                    .is_some_and(|v| !v.eq_ignore_ascii_case("false"))
            })
            .or_else(|| opts.first().copied())
    }

    /// Owned value of the selected option: its `value` attribute, or its
    /// trimmed text when `value` is absent. Empty when nothing is selected.
    pub fn select_value(&self, select_id: NodeId) -> String {
        let Some(opt_id) = self.selected_option(select_id) else {
            return String::new();
        };
        if let Some(val) = self.get(opt_id).data().attr("value") {
            return val.to_string();
        }
        self.text_content(opt_id).trim().to_string()
    }

    /// Owned visible label of the selected option. Falls back to
    /// [`Self::select_value`] when the option's text is empty. Empty when
    /// nothing is selected.
    pub fn select_display_label(&self, select_id: NodeId) -> String {
        let Some(opt_id) = self.selected_option(select_id) else {
            return String::new();
        };
        let text = self.text_content(opt_id).trim().to_string();
        if !text.is_empty() {
            text
        } else {
            self.select_value(select_id)
        }
    }

    /// Mark the next direct option as `selected` and clear the others.
    ///
    /// Wraps to the first option. Also writes the new value onto the `select`.
    /// Returns that owned value. `None` when `select_id` has no option children.
    pub fn cycle_select_option(&mut self, select_id: NodeId) -> Option<String> {
        let opts = self.select_options(select_id);
        if opts.is_empty() {
            return None;
        }
        let current = self.selected_option(select_id);
        let next_idx = match current.and_then(|c| opts.iter().position(|&id| id == c)) {
            Some(idx) => (idx + 1) % opts.len(),
            None => 0,
        };
        for (i, &oid) in opts.iter().enumerate() {
            if i == next_idx {
                self.set_attribute(oid, "selected", "selected");
            } else {
                self.remove_attribute(oid, "selected");
            }
        }
        let new_val = self.select_value(select_id);
        self.set_attribute(select_id, "value", &new_val);
        Some(new_val)
    }

    /// True for `<input type="submit">`, or a `button` whose type is `submit`
    /// or omitted. A missing node is false.
    pub fn is_submit_control(&self, id: NodeId) -> bool {
        let Some(node) = self.nodes.get(id.index()) else {
            return false;
        };
        let data = node.data();
        match data.tag() {
            Some("input") => data
                .attr("type")
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("submit")),
            Some("button") => {
                let kind = data.attr("type").unwrap_or("submit").trim();
                kind.eq_ignore_ascii_case("submit")
            }
            _ => false,
        }
    }

    /// The nearest `form` ancestor, including `id` itself.
    /// `None` if `id` is missing or no ancestor is a form.
    pub fn enclosing_form(&self, id: NodeId) -> Option<NodeId> {
        let mut cursor = Some(id);
        while let Some(curr) = cursor {
            let node = self.nodes.get(curr.index())?;
            if node.data().tag() == Some("form") {
                return Some(curr);
            }
            cursor = node.parent();
        }
        None
    }

    /// Owned `(action, method, urlencoded body)` for a `form` node.
    ///
    /// A missing node yields `("", "GET", "")`. `method` is `POST` only when
    /// the attribute is `POST` ignoring case; every other value, including a
    /// missing one, is `GET`. `action` is the trimmed attribute or empty.
    /// Named `input`, `textarea`, and `select` descendants are included.
    /// Checkboxes and radios are included only when checked, defaulting the
    /// value to `on`. Submit, button, and reset inputs are skipped.
    /// Successful controls are joined with `&` using [`urlencode_component`].
    pub fn serialize_form(&self, form_id: NodeId) -> (String, String, String) {
        let Some(form_node) = self.nodes.get(form_id.index()) else {
            return (String::new(), "GET".to_string(), String::new());
        };
        let action = form_node
            .data()
            .attr("action")
            .unwrap_or("")
            .trim()
            .to_string();
        let method = form_node
            .data()
            .attr("method")
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let method = if method == "POST" {
            "POST".to_string()
        } else {
            "GET".to_string()
        };
        let mut pairs = Vec::new();
        self.collect_form_pairs(form_id, &mut pairs);
        let encoded = pairs
            .into_iter()
            .map(|(k, v)| format!("{}={}", urlencode_component(&k), urlencode_component(&v)))
            .collect::<Vec<_>>()
            .join("&");
        (action, method, encoded)
    }

    /// Replace the children of `id` with one text node, or with nothing when
    /// `text` is empty.
    ///
    /// Previous children stay allocated with `parent` cleared. A missing `id`
    /// is a no-op. `text` is copied.
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        if self.nodes.get(id.index()).is_none() {
            return;
        }
        let old_children = std::mem::take(&mut self.nodes[id.index()].children);
        for child in old_children {
            if let Some(node) = self.nodes.get_mut(child.index()) {
                node.parent = None;
            }
        }
        if !text.is_empty() {
            let text_id = self.create_text(text);
            let _ = self.append(id, text_id);
        }
    }

    /// Make `child` the last child of `parent`.
    ///
    /// Both ids must already be in this document. On success the child is
    /// detached from its previous parent. [`TreeError::Missing`] means an id
    /// is out of range. [`TreeError::Cycle`] means the child is the parent or
    /// an ancestor of the parent. The error leaves the tree unchanged.
    pub fn append(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        if self.nodes.get(parent.index()).is_none() || self.nodes.get(child.index()).is_none() {
            return Err(TreeError::Missing);
        }
        if parent == child {
            return Err(TreeError::Cycle);
        }
        let mut cursor = Some(parent);
        while let Some(id) = cursor {
            if id == child {
                return Err(TreeError::Cycle);
            }
            cursor = self.nodes[id.index()].parent;
        }
        self.detach(child);
        self.nodes[child.index()].parent = Some(parent);
        self.nodes[parent.index()].children.push(child);
        Ok(())
    }

    /// Owned trimmed text of the first `title` element. Empty if there is none.
    pub fn title(&self) -> String {
        let Some(id) = self.find_element(self.root(), "title") else {
            return String::new();
        };
        self.text_content(id).trim().to_string()
    }

    /// Replace the first `title` element's text, or create one under `head`.
    /// `title` is copied.
    pub fn set_title(&mut self, title: &str) {
        let title_id = match self.find_element(self.root(), "title") {
            Some(id) => id,
            None => {
                let id = self.create_element("title", Vec::new());
                let _ = self.append(self.head, id);
                id
            }
        };
        self.set_text_content(title_id, title);
    }

    /// Serialize the live tree as HTML, starting at the `html` element.
    ///
    /// Detached nodes are left out. Text and attribute values are escaped.
    /// The parser decodes those escapes, so a later `parse_html` of this
    /// string restores the text. Comments are not stored on the tree, so they
    /// are not written back.
    pub fn to_html(&self) -> String {
        let mut out = String::from("<!DOCTYPE html>");
        self.write_html(self.html, &mut out);
        out
    }

    /// Write `id` and its live children. The tag string is copied before the
    /// child walk so the recursive borrow can use `self` again.
    fn write_html(&self, id: NodeId, out: &mut String) {
        match self.get(id).data() {
            NodeData::Text(text) => push_escaped(out, text, false),
            NodeData::Document => {
                let kids: Vec<NodeId> = self.children(id).to_vec();
                for child in kids {
                    self.write_html(child, out);
                }
            }
            NodeData::Element { tag, attrs } => {
                let tag = tag.clone();
                let attrs = attrs.clone();
                out.push('<');
                out.push_str(&tag);
                for (name, value) in &attrs {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    push_escaped(out, value, true);
                    out.push('"');
                }
                let kids: Vec<NodeId> = self.children(id).to_vec();
                if kids.is_empty() && is_void_tag(&tag) {
                    out.push('>');
                    return;
                }
                out.push('>');
                for child in kids {
                    self.write_html(child, out);
                }
                out.push_str("</");
                out.push_str(&tag);
                out.push('>');
            }
        }
    }

    /// First element in tree order whose `id` attribute equals `target_id`.
    ///
    /// An empty `target_id` is `None`. The match is case-sensitive. The id is
    /// copied; the document is borrowed.
    pub fn get_element_by_id(&self, target_id: &str) -> Option<NodeId> {
        if target_id.is_empty() {
            return None;
        }
        self.find_by_id(self.root(), target_id)
    }

    /// First element in tree order that matches a simple selector.
    ///
    /// Accepted forms: `tag`, `#id`, `.class`, `tag#id`, `tag.class`.
    /// Whitespace is trimmed. An empty selector is `None`. Combinators are
    /// not supported; a descendant selector does not match. `None` if nothing matches.
    pub fn query_selector(&self, selector: &str) -> Option<NodeId> {
        let selector = selector.trim();
        if selector.is_empty() {
            return None;
        }
        self.find_by_simple_selector(self.root(), selector)
    }

    /// Every element matching [`Self::query_selector`]'s selector grammar,
    /// in tree order. An empty selector yields an empty vec. The ids are owned.
    pub fn query_selector_all(&self, selector: &str) -> Vec<NodeId> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Vec::new();
        }
        let mut matches = Vec::new();
        self.collect_by_simple_selector(self.root(), selector, &mut matches);
        matches
    }

    /// Unlink `id` from its parent. The document node, `html`, `head`, and
    /// `body` are refused. The slot and its descendants stay allocated.
    /// Descendants keep their parent pointers.
    pub fn remove_node(&mut self, id: NodeId) {
        if id == self.root() || id == self.html || id == self.head || id == self.body {
            return;
        }
        self.detach(id);
    }

    /// Replace the children of `id` with a parse of `html_fragment`.
    ///
    /// The fragment is parsed as a document and the new `body` children are
    /// cloned under `id`. Cloning stops at [`MAX_DOM_NODES`]. A missing `id`
    /// is a no-op. Blank markup only detaches the old children. Old slots
    /// stay allocated. `html_fragment` is borrowed.
    pub fn set_inner_html(&mut self, id: NodeId, html_fragment: &str) {
        if self.nodes.get(id.index()).is_none() {
            return;
        }
        let old_children = std::mem::take(&mut self.nodes[id.index()].children);
        for child in old_children {
            if let Some(node) = self.nodes.get_mut(child.index()) {
                node.parent = None;
            }
        }
        if html_fragment.trim().is_empty() {
            return;
        }
        let parsed = parse_html(html_fragment);
        let body_children: Vec<NodeId> = parsed.children(parsed.body()).to_vec();
        for src_child in body_children {
            self.clone_subtree_into(&parsed, src_child, id);
        }
    }

    /// Copy one node from a parsed fragment onto `dst_parent`, then its children.
    ///
    /// `Document` nodes are skipped. Past [`MAX_DOM_NODES`] the copy stops and
    /// the rest of that subtree is dropped. A failed [`Self::append`] also
    /// drops the new node's children. `set_inner_html` uses this to move the
    /// fragment's body children into the live tree.
    fn clone_subtree_into(&mut self, src_doc: &Document, src_id: NodeId, dst_parent: NodeId) {
        // Cloning stops at the same node cap as the parser. The rest of the subtree is dropped.
        if self.nodes.len() >= MAX_DOM_NODES {
            return;
        }
        let new_node = match src_doc.get(src_id).data() {
            NodeData::Text(t) => self.create_text(t),
            NodeData::Element { tag, attrs } => self.create_element(tag, attrs.clone()),
            NodeData::Document => return,
        };
        if self.append(dst_parent, new_node).is_ok() {
            for child in src_doc.children(src_id) {
                self.clone_subtree_into(src_doc, *child, new_node);
            }
        }
    }

    /// Owned text of every `style` element, joined by newlines, in tree order.
    /// A `style` element's descendants are not scanned separately.
    pub fn embedded_css(&self) -> String {
        let mut blocks = Vec::new();
        self.collect_style(self.root(), &mut blocks);
        blocks.join("\n")
    }

    /// Owned text of every `script` element whose text is not only whitespace.
    /// External `src` scripts are not included. See [`Self::external_scripts_with_integrity`].
    pub fn scripts(&self) -> Vec<String> {
        let mut blocks = Vec::new();
        self.collect_scripts(self.root(), &mut blocks);
        blocks
    }

    /// Owned `href` of every stylesheet `<link>`, in tree order.
    /// Integrity metadata is dropped. See [`Self::stylesheet_links_with_integrity`].
    pub fn stylesheet_links(&self) -> Vec<String> {
        self.stylesheet_links_with_integrity()
            .into_iter()
            .map(|(href, _)| href)
            .collect()
    }

    /// Stylesheet links as owned `(href, integrity)`.
    ///
    /// A link counts when `rel` contains the token `stylesheet`. `integrity`
    /// is `None` when the attribute is absent. Empty `href` values are skipped.
    pub fn stylesheet_links_with_integrity(&self) -> Vec<(String, Option<String>)> {
        let mut links = Vec::new();
        self.collect_stylesheet_links(self.root(), &mut links);
        links
    }

    /// External scripts as owned `(src, integrity)`, in tree order.
    /// A script with an empty or missing `src` is skipped. Inline script text
    /// is not included.
    pub fn external_scripts_with_integrity(&self) -> Vec<(String, Option<String>)> {
        let mut scripts = Vec::new();
        self.collect_external_scripts(self.root(), &mut scripts);
        scripts
    }

    /// `<img>` nodes that have a non-empty `src`, as owned `(id, src)` pairs.
    /// The image bytes are not decoded here.
    pub fn images(&self) -> Vec<(NodeId, String)> {
        let mut imgs = Vec::new();
        self.collect_images(self.root(), &mut imgs);
        imgs
    }

    /// `<video>` nodes as owned `(id, src)` pairs.
    ///
    /// Uses the element's `src` when it is non-empty. Otherwise uses the first
    /// child `<source src>`. Videos with neither are omitted. Bytes are not decoded.
    pub fn videos(&self) -> Vec<(NodeId, String)> {
        let mut vids = Vec::new();
        self.collect_videos(self.root(), &mut vids);
        vids
    }

    /// Join every `<meta http-equiv="Content-Security-Policy">` `content`
    /// value with `"; "`. `None` when no such meta element has content.
    /// The string is owned.
    pub fn meta_csp(&self) -> Option<String> {
        let mut out = Vec::new();
        self.collect_meta_csp(self.root(), &mut out);
        if out.is_empty() {
            None
        } else {
            Some(out.join("; "))
        }
    }

    /// Owned concatenation of this node's text and its descendants' text.
    /// A missing id panics through [`Self::get`]. Element tags are not included.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    /// Append a node with no parent. The id is the index in `nodes`.
    /// Callers must check [`MAX_DOM_NODES`] before this. The slot is not reused
    /// after [`Self::detach`].
    fn alloc(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            id,
            parent: None,
            children: Vec::new(),
            data,
        });
        id
    }

    /// Unlink `child` from its parent. The node stays in `nodes`.
    /// A node that already has no parent is left as it is.
    fn detach(&mut self, child: NodeId) {
        let Some(parent) = self.nodes[child.index()].parent else {
            return;
        };
        self.nodes[parent.index()]
            .children
            .retain(|id| *id != child);
        self.nodes[child.index()].parent = None;
    }

    /// Depth-first search for the first element whose tag equals `tag`.
    /// `tag` must already be lowercase. Returns `id` itself when it matches.
    fn find_element(&self, id: NodeId, tag: &str) -> Option<NodeId> {
        if self.get(id).data().tag() == Some(tag) {
            return Some(id);
        }
        for child in self.children(id) {
            if let Some(found) = self.find_element(*child, tag) {
                return Some(found);
            }
        }
        None
    }

    /// Depth-first search for the first node whose `id` attribute equals `target_id`.
    /// The compare is case-sensitive. The first match in tree order wins.
    fn find_by_id(&self, id: NodeId, target_id: &str) -> Option<NodeId> {
        if self.get(id).data().id() == Some(target_id) {
            return Some(id);
        }
        for child in self.children(id) {
            if let Some(found) = self.find_by_id(*child, target_id) {
                return Some(found);
            }
        }
        None
    }

    /// Depth-first search using [`matches_simple_selector`]. First match wins.
    fn find_by_simple_selector(&self, id: NodeId, selector: &str) -> Option<NodeId> {
        if matches_simple_selector(self.get(id).data(), selector) {
            return Some(id);
        }
        for child in self.children(id) {
            if let Some(found) = self.find_by_simple_selector(*child, selector) {
                return Some(found);
            }
        }
        None
    }

    /// Push every match of `selector` under `id`, including `id`, in tree order.
    fn collect_by_simple_selector(&self, id: NodeId, selector: &str, out: &mut Vec<NodeId>) {
        if matches_simple_selector(self.get(id).data(), selector) {
            out.push(id);
        }
        for child in self.children(id) {
            self.collect_by_simple_selector(*child, selector, out);
        }
    }

    /// Append this node's text, then each child's text, with no separator.
    fn collect_text(&self, id: NodeId, out: &mut String) {
        if let Some(text) = self.get(id).data().text() {
            out.push_str(text);
        }
        for child in self.children(id) {
            self.collect_text(*child, out);
        }
    }

    /// Collect non-empty `style` element text. A `style` element's children are
    /// not walked again, so a nested element inside `style` is not a second block.
    fn collect_style(&self, id: NodeId, out: &mut Vec<String>) {
        if self.get(id).data().tag() == Some("style") {
            let css = self.text_content(id);
            if !css.is_empty() {
                out.push(css);
            }
            return;
        }
        for child in self.children(id) {
            self.collect_style(*child, out);
        }
    }

    /// Collect inline `script` text that is not only whitespace.
    /// A script element's children are not walked again. `src` scripts are
    /// still visited here for their text; [`Self::collect_external_scripts`]
    /// records the URL separately. `anvil-script` runs the strings this returns.
    fn collect_scripts(&self, id: NodeId, out: &mut Vec<String>) {
        if self.get(id).data().tag() == Some("script") {
            let code = self.text_content(id);
            if !code.trim().is_empty() {
                out.push(code);
            }
            return;
        }
        for child in self.children(id) {
            self.collect_scripts(*child, out);
        }
    }

    /// Record `link rel=stylesheet` as `(href, integrity)` and keep walking.
    /// `rel` matches the token `stylesheet`, ignoring case. Empty `href` is skipped.
    /// `anvil-browser` fetches these URLs and appends the CSS before `anvil-style` runs.
    fn collect_stylesheet_links(&self, id: NodeId, out: &mut Vec<(String, Option<String>)>) {
        let data = self.get(id).data();
        if data.tag() == Some("link") {
            let is_stylesheet = data.attr("rel").is_some_and(|rel| {
                rel.split_ascii_whitespace()
                    .any(|token| token.eq_ignore_ascii_case("stylesheet"))
            });
            if is_stylesheet {
                if let Some(href) = data.attr("href") {
                    let href = href.trim();
                    if !href.is_empty() {
                        let integrity = data.attr("integrity").map(|s| s.trim().to_string());
                        out.push((href.to_string(), integrity));
                    }
                }
            }
        }
        for child in self.children(id) {
            self.collect_stylesheet_links(*child, out);
        }
    }

    /// Record `script src` as `(src, integrity)` and keep walking into children.
    /// Empty `src` is skipped. `anvil-browser` fetches the URL, checks SRI when
    /// the integrity string is present, then passes the body to `anvil-script`.
    fn collect_external_scripts(&self, id: NodeId, out: &mut Vec<(String, Option<String>)>) {
        let data = self.get(id).data();
        if data.tag() == Some("script") {
            if let Some(src) = data.attr("src") {
                let src = src.trim();
                if !src.is_empty() {
                    let integrity = data.attr("integrity").map(|s| s.trim().to_string());
                    out.push((src.to_string(), integrity));
                }
            }
        }
        for child in self.children(id) {
            self.collect_external_scripts(*child, out);
        }
    }

    /// Record `img` with a non-empty `src`. Does not decode bytes.
    /// `anvil-browser` fetches the URL and calls `anvil-image`. `anvil-layout`
    /// later reads the decoded width and height.
    fn collect_images(&self, id: NodeId, out: &mut Vec<(NodeId, String)>) {
        let data = self.get(id).data();
        if data.tag() == Some("img") {
            if let Some(src) = data.attr("src") {
                let src = src.trim();
                if !src.is_empty() {
                    out.push((id, src.to_string()));
                }
            }
        }
        for child in self.children(id) {
            self.collect_images(*child, out);
        }
    }

    /// Record one URL per `video`: its `src`, or else the first child `source src`.
    /// Nodes with neither are omitted. Bytes stay undecoded. `anvil-browser`
    /// passes the fetched body to `anvil-video`.
    fn collect_videos(&self, id: NodeId, out: &mut Vec<(NodeId, String)>) {
        let data = self.get(id).data();
        if data.tag() == Some("video") {
            if let Some(src) = data.attr("src") {
                let src = src.trim();
                if !src.is_empty() {
                    out.push((id, src.to_string()));
                }
            } else {
                for child in self.children(id) {
                    let cdata = self.get(*child).data();
                    if cdata.tag() == Some("source") {
                        if let Some(src) = cdata.attr("src") {
                            let src = src.trim();
                            if !src.is_empty() {
                                out.push((id, src.to_string()));
                                break;
                            }
                        }
                    }
                }
            }
        }
        for child in self.children(id) {
            self.collect_videos(*child, out);
        }
    }

    /// Record `meta http-equiv=Content-Security-Policy` content values.
    /// The compare on `http-equiv` ignores case. Empty content is skipped.
    /// `anvil-net` parses the joined string as a content security policy.
    fn collect_meta_csp(&self, id: NodeId, out: &mut Vec<String>) {
        let data = self.get(id).data();
        if data.tag() == Some("meta") {
            let is_csp = data
                .attr("http-equiv")
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("content-security-policy"));
            if is_csp {
                if let Some(content) = data.attr("content") {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        out.push(trimmed.to_string());
                    }
                }
            }
        }
        for child in self.children(id) {
            self.collect_meta_csp(*child, out);
        }
    }

    /// Walk form controls and push `(name, value)` pairs for a submit body.
    ///
    /// Skips controls with an empty `name`. Checkboxes and radios contribute
    /// only when checked (`value`, or `on` when `value` is absent). `submit`,
    /// `button`, and `reset` inputs are skipped. Other inputs use the `value`
    /// attribute. A `textarea` uses [`Self::input_value`]. A `select` uses the
    /// selected option. `anvil-browser` encodes the pairs for `anvil-net` POST.
    fn collect_form_pairs(&self, id: NodeId, out: &mut Vec<(String, String)>) {
        let data = self.get(id).data();
        match data.tag() {
            Some("input") => {
                if let Some(name) = data.attr("name") {
                    let name = name.trim();
                    if !name.is_empty() {
                        let kind = data
                            .attr("type")
                            .unwrap_or("text")
                            .trim()
                            .to_ascii_lowercase();
                        match kind.as_str() {
                            "checkbox" | "radio" => {
                                if self.is_checked(id) {
                                    let val = data.attr("value").unwrap_or("on");
                                    out.push((name.to_string(), val.to_string()));
                                }
                            }
                            "submit" | "button" | "reset" => {}
                            _ => {
                                let val = data.attr("value").unwrap_or("");
                                out.push((name.to_string(), val.to_string()));
                            }
                        }
                    }
                }
            }
            Some("textarea") => {
                if let Some(name) = data.attr("name") {
                    let name = name.trim();
                    if !name.is_empty() {
                        out.push((name.to_string(), self.input_value(id)));
                    }
                }
            }
            Some("select") => {
                if let Some(name) = data.attr("name") {
                    let name = name.trim();
                    if !name.is_empty() {
                        out.push((name.to_string(), self.select_value(id)));
                    }
                }
            }
            _ => {}
        }
        for child in self.children(id) {
            self.collect_form_pairs(*child, out);
        }
    }
}

/// Encode one form field for `application/x-www-form-urlencoded`.
///
/// `input` is borrowed. The result is owned. Unreserved ASCII stays as-is.
/// A space becomes `+`. Every other byte becomes `%XX` using uppercase hex.
pub fn urlencode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Match one simple selector against borrowed node data.
///
/// Forms: `#id`, `.class`, `tag#id`, `tag.class`, or a tag name. A leading
/// empty tag (`#id` is handled first) is allowed in the split forms. Text and
/// the document node return false. Class and id compares are case-sensitive.
/// The tag compare ignores ASCII case.
/// Escape `&`, `<`, and `>` in text. Attribute values also escape `"`.
fn push_escaped(out: &mut String, text: &str, attr: bool) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// Tags that have no end tag in the serializer when they have no children.
fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn matches_simple_selector(data: &NodeData, selector: &str) -> bool {
    let Some(actual_tag) = data.tag() else {
        return false;
    };
    if let Some(id_name) = selector.strip_prefix('#') {
        return data.id() == Some(id_name);
    }
    if let Some(class_name) = selector.strip_prefix('.') {
        return data.has_class(class_name);
    }
    if let Some((tag_part, id_name)) = selector.split_once('#') {
        return (tag_part.is_empty() || actual_tag.eq_ignore_ascii_case(tag_part))
            && data.id() == Some(id_name);
    }
    if let Some((tag_part, class_name)) = selector.split_once('.') {
        return (tag_part.is_empty() || actual_tag.eq_ignore_ascii_case(tag_part))
            && data.has_class(class_name);
    }
    actual_tag.eq_ignore_ascii_case(selector)
}

#[cfg(test)]
mod tests {
    //! Parser caps, selectors, forms, and cycle checks against a real document.

    use super::*;

    #[test]
    fn append_refuses_a_cycle() {
        let mut doc = Document::new();
        let div = doc.create_element("div", Vec::new());
        doc.append(doc.body(), div).unwrap();
        assert_eq!(doc.append(div, doc.body()), Err(TreeError::Cycle));
    }

    #[test]
    fn reads_text_attributes_and_title() {
        let doc = parse_html(
            "<title>Hello</title><p class=\"note\">A &amp; B</p><!-- hidden --><br><span>C</span>",
        );
        assert_eq!(doc.title(), "Hello");
        let paragraph = doc
            .children(doc.body())
            .iter()
            .copied()
            .find(|id| doc.get(*id).data().tag() == Some("p"))
            .unwrap();
        assert_eq!(doc.get(paragraph).data().attr("class"), Some("note"));
        assert_eq!(doc.text_content(paragraph), "A & B");
        let again = parse_html(&doc.to_html());
        let again_p = again
            .children(again.body())
            .iter()
            .copied()
            .find(|id| again.get(*id).data().tag() == Some("p"))
            .expect("paragraph");
        assert_eq!(again.text_content(again_p), "A & B");
        assert_eq!(again.get(again_p).data().attr("class"), Some("note"));
        let tags: Vec<_> = doc
            .children(doc.body())
            .iter()
            .filter_map(|id| doc.get(*id).data().tag())
            .collect();
        assert_eq!(tags, ["p", "br", "span"]);
    }

    #[test]
    fn serializes_replaced_text() {
        let mut doc = parse_html("<p id=\"out\">secret-marker</p>");
        let id = doc.get_element_by_id("out").unwrap();
        doc.set_text_content(id, "from-worker");
        let again = parse_html(&doc.to_html());
        let id2 = again.get_element_by_id("out").unwrap();
        assert_eq!(again.text_content(id2), "from-worker");
    }

    #[test]
    fn script_text_keeps_a_less_than() {
        let doc = parse_html("<script>if (a < b) {}</script><p>Hi</p>");
        let script = doc.find_element(doc.root(), "script").unwrap();
        assert_eq!(doc.text_content(script), "if (a < b) {}");
        assert_eq!(doc.scripts(), vec!["if (a < b) {}"]);
        assert!(doc.find_element(doc.body(), "p").is_some());
    }

    #[test]
    fn collects_stylesheet_links_in_order() {
        let doc = parse_html(
            "<head>
                <link rel=\"icon\" href=\"favicon.ico\">
                <link rel=\"stylesheet\" href=\"base.css\">
                <link rel=\"alternate stylesheet\" href=\"theme.css\">
             </head>",
        );
        assert_eq!(doc.stylesheet_links(), vec!["base.css", "theme.css"]);
    }

    #[test]
    fn queries_and_mutates_elements_by_id_and_selector() {
        let mut doc = parse_html("<div id=\"banner\" class=\"card primary\">Old</div>");
        let id = doc.get_element_by_id("banner").unwrap();
        assert_eq!(doc.query_selector(".primary"), Some(id));
        assert_eq!(doc.query_selector("div.card"), Some(id));
        doc.set_text_content(id, "Updated");
        doc.set_attribute(id, "data-state", "active");
        assert_eq!(doc.text_content(id), "Updated");
        assert_eq!(doc.get(id).data().attr("data-state"), Some("active"));
    }

    #[test]
    fn serializes_form_inputs_checkboxes_and_toggles_classes() {
        let mut doc = parse_html(
            "<form id=\"search-form\" action=\"/search\" method=\"POST\">
                <input id=\"q\" type=\"text\" name=\"query\" value=\"anvil engine\">
                <input id=\"exact\" type=\"checkbox\" name=\"exact\" value=\"1\" checked>
                <button id=\"go\" type=\"submit\">Search</button>
             </form>",
        );
        let q = doc.get_element_by_id("q").unwrap();
        let exact = doc.get_element_by_id("exact").unwrap();
        let go = doc.get_element_by_id("go").unwrap();
        let form = doc.enclosing_form(go).unwrap();

        assert!(doc.is_text_input(q));
        assert!(doc.is_checkbox_input(exact));
        assert!(doc.is_submit_control(go));

        let (action, method, body) = doc.serialize_form(form);
        assert_eq!(action, "/search");
        assert_eq!(method, "POST");
        assert_eq!(body, "query=anvil+engine&exact=1");

        doc.set_input_value(q, "a&b");
        assert!(!doc.toggle_checked(exact));
        let (_, _, body2) = doc.serialize_form(form);
        assert_eq!(body2, "query=a%26b");

        assert!(doc.toggle_class(form, "submitted"));
        assert!(doc.get(form).data().has_class("submitted"));
        assert!(!doc.toggle_class(form, "submitted"));
        assert!(!doc.get(form).data().has_class("submitted"));
    }

    #[test]
    fn caps_adversarial_dom_nesting_depth_and_attribute_counts() {
        let mut deep_html = String::new();
        for _ in 0..400 {
            deep_html.push_str("<div>");
        }
        deep_html.push_str("<p id=\"leaf\">Safe Leaf</p>");
        let doc = parse_html(&deep_html);
        let leaf = doc.get_element_by_id("leaf").unwrap();
        let mut depth = 0usize;
        let mut cursor = Some(leaf);
        while let Some(curr) = cursor {
            depth += 1;
            cursor = doc.get(curr).parent();
        }
        assert!(depth <= MAX_DOM_DEPTH + 4);
    }

    #[test]
    fn parses_tables_textareas_and_cycles_select_options() {
        let mut doc = parse_html(
            "<table id=\"tbl\">
               <tr><th>Col A<th>Col B
               <tr><td>1<td>2
             </table>
             <form id=\"f\" action=\"/save\" method=\"POST\">
               <textarea id=\"bio\" name=\"bio\">Line 1\nLine 2</textarea>
               <select id=\"tier\" name=\"tier\">
                 <option value=\"free\">Free Tier
                 <option value=\"pro\" selected>Pro Tier
                 <option value=\"max\">Max Tier
               </select>
             </form>",
        );
        let tbl = doc.get_element_by_id("tbl").unwrap();
        assert_eq!(doc.children(tbl).len(), 2);

        let tier = doc.get_element_by_id("tier").unwrap();
        assert!(doc.is_select_control(tier));
        assert_eq!(doc.select_value(tier), "pro");
        assert_eq!(doc.select_display_label(tier), "Pro Tier");

        assert_eq!(doc.cycle_select_option(tier), Some("max".to_string()));
        assert_eq!(doc.cycle_select_option(tier), Some("free".to_string()));

        let form = doc.get_element_by_id("f").unwrap();
        let (_, _, payload) = doc.serialize_form(form);
        assert_eq!(payload, "bio=Line+1+Line+2&tier=free");
    }
}
