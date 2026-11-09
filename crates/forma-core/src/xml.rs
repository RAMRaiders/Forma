//! A small, tolerant XML tree reader for Batocera's config and gamelist files.
//!
//! These files are written by many tools (EmulationStation, scrapers, hand edits) and are
//! sometimes truncated by a power cut mid-write. Instead of failing the whole file, the reader
//! keeps every element that closed before the error and reports the error alongside it.

use quick_xml::escape::{resolve_html5_entity, resolve_predefined_entity};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Default)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Trimmed text of the first child called `name`, if it has any.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|c| c.text.trim())
            .filter(|t| !t.is_empty())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

pub(crate) struct Parsed {
    /// The document element, holding every child that closed before any error.
    pub root: Option<Element>,
    /// Why parsing stopped early, if it did.
    pub error: Option<String>,
}

pub(crate) fn parse(xml: &str) -> Parsed {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    let error = loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(open(&e)),
            Ok(Event::Empty(e)) => close(open(&e), &mut stack, &mut root),
            Ok(Event::End(_)) => {
                if let Some(done) = stack.pop() {
                    close(done, &mut stack, &mut root);
                }
            }
            Ok(Event::Text(t)) => push_text(&mut stack, &t.xml10_content()),
            Ok(Event::CData(t)) => push_text(&mut stack, &t.xml10_content()),
            Ok(Event::GeneralRef(r)) => push_text(&mut stack, &resolve_ref(&r)),
            Ok(Event::Eof) => {
                break (!stack.is_empty()).then(|| "file ends inside an element".to_owned());
            }
            Ok(_) => {}
            Err(e) => break Some(format!("{e} at byte {}", reader.error_position())),
        }
    };

    // Elements still open were cut off: keep the document element with what it holds, drop the rest.
    if root.is_none() && !stack.is_empty() {
        root = Some(stack.swap_remove(0));
    }
    Parsed { root, error }
}

fn open(e: &BytesStart<'_>) -> Element {
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let value = a
                .normalized_value(XmlVersion::Implicit1_0)
                .map_or_else(|_| a.value.to_string(), |v| v.to_string());
            (a.key.as_ref().to_owned(), value)
        })
        .collect();
    Element {
        name: e.name().as_ref().to_owned(),
        attrs,
        ..Element::default()
    }
}

fn close(done: Element, stack: &mut [Element], root: &mut Option<Element>) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(done),
        None => *root = Some(done),
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    if let Some(current) = stack.last_mut() {
        current.text.push_str(text);
    }
}

/// XML's five entities and character references, plus HTML5 names (`&nbsp;`, `&eacute;`) that
/// scrapers copy from web pages. An unknown name is kept as written rather than dropped.
pub(crate) fn resolve_ref(r: &BytesRef<'_>) -> String {
    if r.is_char_ref()
        && let Ok(Some(c)) = r.resolve_char_ref()
    {
        return c.to_string();
    }
    let name: &str = r;
    resolve_predefined_entity(name)
        .or_else(|| resolve_html5_entity(name))
        .map_or_else(|| format!("&{name};"), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_elements_text_and_attributes() {
        let p = parse(r#"<a x="1"><b>hi</b><c/></a>"#);
        assert!(p.error.is_none());
        let a = p.root.unwrap();
        assert_eq!(a.attr("x"), Some("1"));
        assert_eq!(a.child_text("b"), Some("hi"));
        assert!(a.child("c").is_some());
    }

    #[test]
    fn resolves_xml_char_and_html_entities_and_keeps_unknown_ones() {
        let p = parse("<a>Tom &amp; Jerry &#233; &nbsp;x &bogus;</a>");
        assert_eq!(p.root.unwrap().text, "Tom & Jerry é \u{a0}x &bogus;");
    }

    #[test]
    fn keeps_cdata_verbatim() {
        let p = parse("<a><![CDATA[<b>not a tag</b>]]></a>");
        assert_eq!(p.root.unwrap().text, "<b>not a tag</b>");
    }

    #[test]
    fn a_truncated_file_keeps_what_closed_and_drops_the_cut_element() {
        let p = parse("<list><item>one</item><item>tw");
        assert!(p.error.is_some());
        let list = p.root.unwrap();
        assert_eq!(list.children_named("item").count(), 1);
        assert_eq!(list.child_text("item"), Some("one"));
    }

    #[test]
    fn a_mismatched_end_tag_stops_but_keeps_earlier_elements() {
        let p = parse("<list><item>one</item><item>two</wrong></list>");
        assert!(p.error.is_some());
        assert_eq!(p.root.unwrap().children_named("item").count(), 1);
    }
}
