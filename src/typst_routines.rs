use typst::{
    Features, Library, LibraryExt as _,
    diag::SourceDiagnostic,
    ecow::EcoVec,
    foundations::{
        Content, Element, Label, NativeFunc as _, Recipe, Selector, Style, Transformation, Value,
    },
    introspection::Introspector as _,
    model::RefElem,
    syntax::{Span, VirtualPath},
    utils::LazyHash,
};

use crate::typst_world::{GlobalContext, TemporaryWorld};

#[typst_macros::func]
fn ignore_refs(_body: Content) -> Result<Value, typst::ecow::EcoVec<SourceDiagnostic>> {
    Ok(Value::None)
}

pub fn collect_labels(
    context: &GlobalContext,
    paths: impl Iterator<Item = VirtualPath>,
) -> (Vec<(VirtualPath, Label)>, EcoVec<SourceDiagnostic>) {
    let mut labels = Vec::new();
    let mut diagnostics = EcoVec::new();

    let mut library = Library::builder().with_features(Features::all()).build();
    library.styles.push(Style::Recipe(Recipe::new(
        Some(Selector::Elem(Element::of::<RefElem>(), Default::default())),
        Transformation::Func(ignore_refs::func()),
        Span::detached(),
    )));
    let library = LazyHash::new(library);

    for path in paths {
        let world = TemporaryWorld {
            main: &path,
            library: &library,
            context,
        };

        match typst::compile::<typst_html::HtmlDocument>(&world).output {
            Ok(document) => labels.extend(
                document
                    .introspector()
                    .query_labelled()
                    .into_iter()
                    .filter_map(|elem| elem.label())
                    .map(|label| (path.clone(), label)),
            ),
            Err(diag) => diagnostics.extend(diag),
        };
    }

    (labels, diagnostics)
}
