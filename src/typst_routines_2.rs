use comemo::Tracked;
use rustc_hash::{FxHashMap, FxHashSet};
use typst::{
    World as _,
    diag::{HintedString, Severity, SourceDiagnostic, error},
    ecow::{EcoString, EcoVec},
    engine::Engine,
    foundations::{
        BundlePath, Bytes, Content, Context, IntoValue as _, Label, LocatableSelector,
        NativeElement as _, PathOrStr, Repr, Selector, Value, eco_format,
    },
    introspection::{Introspector, QueryIntrospection},
    model::{AssetData, AssetElem, DocumentElem},
    syntax::{Spanned, VirtualPath, VirtualRoot},
};
use typst_macros::func;

use crate::typst_wiki::GlobalIndicator;

#[typst_macros::ty(scope)]
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct WikiScope;

impl Repr for WikiScope {
    fn repr(&self) -> EcoString {
        "wiki".into()
    }
}

#[typst_macros::scope]
impl WikiScope {
    #[func]
    fn query(
        &self,
        engine: &mut Engine,
        context: Tracked<Context>,
        selector: Spanned<Selector>,
    ) -> Result<Vec<Content>, HintedString> {
        let global_selector = selector
            .v
            .within(LocatableSelector(Selector::can::<GlobalIndicator>()));

        context.introspect()?;
        Ok(engine
            .introspect(QueryIntrospection(global_selector, selector.span))
            .into_iter()
            .collect())
    }

    #[func]
    fn query_label(
        &self,
        engine: &mut Engine,
        context: Tracked<Context>,
        label: Spanned<Label>,
    ) -> Result<Value, HintedString> {
        let mut queried = self.query(
            engine,
            context,
            Spanned {
                v: Selector::Label(label.v),
                span: label.span,
            },
        )?;

        match queried.pop() {
            None => Err(error!(
                "label `<{}>` does not exist in the wiki",
                label.v.resolve()
            )),
            Some(_) if !queried.is_empty() => Err(error!(
                "label `<{}>` occurs multiple times in the wiki",
                label.v.resolve()
            )),
            Some(ext_ref) => Ok(ext_ref.into_value()),
        }
    }

    #[func]
    fn read_asset(
        &self,
        engine: &Engine,
        path: Spanned<PathOrStr>,
        #[default] anchor: Option<Content>,
    ) -> Result<Value, HintedString> {
        let anchor_span = anchor
            .map(|content| content.span())
            .unwrap_or_else(|| path.span);
        let resolved = path
            .v
            .resolve(anchor_span.id().ok_or("the containing file is unknown")?)?;
        if resolved.root() != engine.world.main().root() {
            return Err("including resources from outside the main root is not supported".into());
        }

        let content = engine.world.file(resolved.clone().intern())?;

        Ok(AssetElem::new(
            BundlePath::new(resolved.vpath().clone())?,
            AssetData(content),
        )
        .into_value())
    }

    #[func]
    fn path_of(&self, anchor: Content) -> Result<Value, HintedString> {
        let file_id = anchor.span().id().ok_or("the containing file is unknown")?;
        Ok(PathOrStr::Path(file_id.get().to_owned()).into_value())
    }

    #[func]
    fn make_relative(&self, engine: &Engine, path: PathOrStr) -> Result<EcoString, HintedString> {
        todo!()
    }
}

pub fn collect_document_paths(introspector: impl Introspector) -> FxHashSet<VirtualPath> {
    introspector
        .query(&Selector::Elem(DocumentElem::ELEM, None))
        .into_iter()
        .filter_map(|content| content.span().id())
        .filter_map(|id| {
            if *id.root() == VirtualRoot::Project {
                Some(id.vpath().clone())
            } else {
                None
            }
        })
        .collect()
}

pub fn collect_assets(
    introspector: impl Introspector,
) -> Result<Vec<(VirtualPath, Bytes)>, Vec<SourceDiagnostic>> {
    let mut potential_outputs = FxHashMap::default();

    for (span, asset) in introspector
        .query(&Selector::Elem(AssetElem::ELEM, None))
        .into_iter()
        .filter_map(|content| content.into_packed::<AssetElem>().ok())
        .map(|packed| (packed.span(), packed.unpack()))
    {
        potential_outputs
            .entry(asset.path.into_inner())
            .or_insert_with(Vec::new)
            .push((span, asset.data));
    }

    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    for (path, mut candidates) in potential_outputs.drain() {
        let (first_span, first_data) = candidates.pop().unwrap_or_else(|| unreachable!());
        let conflict_messages: EcoVec<_> = candidates
            .into_iter()
            .filter(|(_, data)| *data != first_data)
            .map(|(span, _)| Spanned::new("conflicting asset".into(), span.into()))
            .collect();

        if !conflict_messages.is_empty() {
            errors.push(SourceDiagnostic {
                severity: Severity::Error,
                span: first_span.into(),
                message: eco_format!(
                    r#"multiple conflicting assets for output "{}""#,
                    path.get_with_slash()
                ),
                trace: Default::default(),
                hints: conflict_messages,
            });
        } else {
            outputs.push((path, first_data.0))
        }
    }

    if errors.is_empty() {
        Ok(outputs)
    } else {
        Err(errors)
    }
}
