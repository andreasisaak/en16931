#![cfg(any(feature = "ubl", feature = "cii"))]

//! The writer emits elements in UBL's schema order. A wrong order is
//! schema-invalid and no Schematron rule reports it. The expected order is
//! [`en16931_formats::ubl::order`], derived from published instances.

#![cfg(feature = "ubl")]

mod common;

macro_rules! syntaxes {
    ($($feature:literal => $module:ident, $syntax:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::common;
            use en16931_formats::$module::order;
            use en16931_formats::Syntax;

            /// Every `(parent, children)` adjacency in a document.
            fn walk(doc: &roxmltree::Document<'_>) -> Vec<(String, Vec<String>)> {
                fn rec(n: roxmltree::Node<'_, '_>, out: &mut Vec<(String, Vec<String>)>) {
                    let kids: Vec<String> = n
                        .children()
                        .filter(roxmltree::Node::is_element)
                        .map(|c| c.tag_name().name().to_owned())
                        .collect();
                    if !kids.is_empty() {
                        out.push((n.tag_name().name().to_owned(), kids));
                    }
                    for c in n.children().filter(roxmltree::Node::is_element) {
                        rec(c, out);
                    }
                }
                let mut out = Vec::new();
                rec(doc.root_element(), &mut out);
                out
            }

            fn check(inv: &en16931::Invoice) {
                let xml = en16931_formats::write_unchecked(inv, Syntax::$syntax)
                    .expect("the fixture carries everything the schema requires")
                    .xml;
                let doc = roxmltree::Document::parse(&xml)
                    .expect("the writer emits well-formed XML");
                let mut checked = 0usize;
                for (parent, kids) in walk(&doc) {
                    let Some(expected) = order::children_of(&parent) else {
                        continue;
                    };
                    let mut last: Option<(usize, &str)> = None;
                    for k in &kids {
                        let Some(i) = expected.iter().position(|e| e == k) else {
                            panic!(
                                "<{parent}> has child <{k}>, which no authority instance carries"
                            );
                        };
                        if let Some((j, prev)) = last {
                            assert!(
                                i >= j,
                                "<{parent}>: <{k}> must come before <{prev}>\n\
                                 expected: {expected:?}\ngot: {kids:?}"
                            );
                        }
                        last = Some((i, k));
                        checked += 1;
                    }
                }
                assert!(
                    checked > 80,
                    "only {checked} elements checked — the fixture is too thin"
                );
            }

            #[test]
            fn an_invoice_is_written_in_schema_order() {
                check(&common::maximal());
            }

            #[test]
            fn a_credit_note_is_written_in_schema_order() {
                check(&common::maximal_credit_note());
            }

            /// The table must stay sorted for `children_of`'s binary search.
            #[test]
            fn the_table_is_sorted_and_findable() {
                let mut sorted = order::ORDER.to_vec();
                sorted.sort_by_key(|(p, _)| *p);
                assert_eq!(sorted, order::ORDER, "ORDER must be sorted");
                assert_eq!(order::children_of("NoSuchElement"), None);
                assert!(!order::ORDER.is_empty());
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl, Ubl,
    "cii" => cii, Cii,
}
