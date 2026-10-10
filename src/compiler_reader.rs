//! Select CLJC branches structurally, retaining original node locations.
use crate::{AnalysisError, AnalysisErrorKind, CollectionKind, NodeId, NodeKind, SyntaxTree};

pub(crate) fn select_cljs(
    source: &str,
    tree: &mut SyntaxTree,
    discards: &mut Vec<NodeId>,
) -> Result<(), AnalysisError> {
    struct Selection<'a> {
        source: &'a str,
        tree: &'a mut SyntaxTree,
        suppressed: Vec<std::ops::Range<usize>>,
    }
    impl Selection<'_> {
        fn error(&self, id: NodeId, message: &'static str) -> AnalysisError {
            AnalysisError {
                span: self.tree.nodes[id.0].span.clone(),
                kind: AnalysisErrorKind::InvalidForm(message),
            }
        }
        fn one(&mut self, id: NodeId) -> Result<NodeId, AnalysisError> {
            let nodes = self.form(id, false)?;
            if nodes.len() != 1 {
                return Err(self.error(id, "reader prefix requires one selected form"));
            }
            Ok(nodes[0])
        }
        fn forms(
            &mut self,
            ids: &[NodeId],
            collection: bool,
        ) -> Result<Vec<NodeId>, AnalysisError> {
            let mut result = Vec::new();
            for &id in ids {
                result.extend(self.form(id, collection)?);
            }
            Ok(result)
        }
        fn form(&mut self, id: NodeId, collection: bool) -> Result<Vec<NodeId>, AnalysisError> {
            let kind = match self.tree.nodes[id.0].kind.clone() {
                NodeKind::Conditional { splicing, branches } => {
                    let NodeKind::Collection { children, .. } =
                        self.tree.nodes[branches.0].kind.clone()
                    else {
                        unreachable!()
                    };
                    let mut selected = None;
                    for pair in children.as_chunks::<2>().0.iter() {
                        let feature = &self.source[self.tree.nodes[pair[0].0].span.clone()];
                        if selected.is_none() && matches!(feature, ":cljs" | ":default") {
                            selected = Some(pair[1]);
                        } else {
                            self.suppressed
                                .push(self.tree.nodes[pair[1].0].span.clone());
                        }
                    }
                    let Some(selected) = selected else {
                        return Ok(vec![]);
                    };
                    let selected = self.one(selected)?;
                    if !splicing {
                        return Ok(vec![selected]);
                    }
                    if !collection {
                        return Err(
                            self.error(id, "reader conditional splicing requires a collection")
                        );
                    }
                    return match self.tree.nodes[selected.0].kind.clone() {
                        NodeKind::Collection {
                            kind: CollectionKind::List | CollectionKind::Vector,
                            children,
                        } => Ok(children),
                        _ => Err(self.error(
                            id,
                            "reader conditional splicing requires a sequential value",
                        )),
                    };
                }
                NodeKind::Collection { kind, children } => {
                    let children = self.forms(&children, true)?;
                    if kind == CollectionKind::Map && !children.len().is_multiple_of(2) {
                        return Err(self.error(id, "map requires an even number of selected forms"));
                    }
                    NodeKind::Collection { kind, children }
                }
                NodeKind::Prefix { kind, form } => NodeKind::Prefix {
                    kind,
                    form: self.one(form)?,
                },
                NodeKind::Metadata { metadata, target } => NodeKind::Metadata {
                    metadata: self.one(metadata)?,
                    target: self.one(target)?,
                },
                NodeKind::Tagged { tag, form } => NodeKind::Tagged {
                    tag,
                    form: self.one(form)?,
                },
                NodeKind::NamespacedMap {
                    auto_resolve,
                    namespace,
                    map,
                } => NodeKind::NamespacedMap {
                    auto_resolve,
                    namespace,
                    map: self.one(map)?,
                },
                kind => kind,
            };
            self.tree.nodes[id.0].kind = kind;
            Ok(vec![id])
        }
    }
    let roots = tree.roots.clone();
    let mut selection = Selection {
        source,
        tree,
        suppressed: vec![],
    };
    selection.tree.roots = selection.forms(&roots, false)?;
    let mut selected_discards = Vec::new();
    for &id in discards.iter() {
        if !selection
            .suppressed
            .iter()
            .any(|span| span.contains(&selection.tree.nodes[id.0].span.start))
        {
            selected_discards.extend(selection.form(id, false)?);
        }
    }
    *discards = selected_discards;
    Ok(())
}
