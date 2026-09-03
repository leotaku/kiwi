use std::sync::LazyLock;

use comemo::Tracked;
use rustc_hash::FxHashSet;
use typst::{
    World as _,
    diag::{HintedString, error},
    ecow::EcoString,
    engine::Engine,
    foundations::{
        BundlePath, Content, Context, IntoValue as _, Label, LocatableSelector, NativeElement as _,
        PathOrStr, Recipe, Repr, Selector, Str, Transformation, Value, eco_format,
    },
    introspection::{Introspector, Location, MetadataElem, QueryIntrospection},
    model::{AssetData, AssetElem},
    syntax::{Span, Spanned, VirtualPath},
};
use typst_macros::func;

use crate::typst_wiki::GlobalQueryMarker;

pub static HIDE_ASSETS_RECIPE: LazyLock<Recipe> = LazyLock::new(|| {
    Recipe::new(
        Some(AssetElem::ELEM.select()),
        Transformation::Content(Content::empty()),
        Span::detached(),
    )
});

#[typst_macros::ty]
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct IndexMarker(pub VirtualPath);

impl Repr for IndexMarker {
    fn repr(&self) -> EcoString {
        eco_format!("index-marker({})", self.0.get_with_slash().repr())
    }
}

pub fn collect_index_paths<I: Introspector>(introspector: &I) -> FxHashSet<VirtualPath> {
    introspector
        .query(&Selector::Elem(MetadataElem::ELEM, None))
        .into_iter()
        .filter_map(|content| content.into_packed::<MetadataElem>().ok())
        .filter_map(|elem| elem.value.clone().cast::<IndexMarker>().ok())
        .map(|value| value.0)
        .collect()
}

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
            .within(LocatableSelector(Selector::can::<GlobalQueryMarker>()));

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
    fn input_of(&self, anchor: Content) -> Result<Str, HintedString> {
        let file_id = anchor.span().id().ok_or("the containing file is unknown")?;
        Ok(file_id.get().vpath().get_with_slash().into())
    }

    #[func]
    fn document_of(&self, engine: &Engine, location: Location) -> Result<Location, HintedString> {
        engine
            .introspector
            .access("foo")
            .document(location)
            .ok_or_else(|| "TODO".into())
    }

    #[func]
    fn make_relative(&self, path: Str, base: Str) -> Result<EcoString, HintedString> {
        let path = VirtualPath::new(path).unwrap();
        let base = VirtualPath::new(base).unwrap();

        Ok(relative_from_parent(&path, &base))
    }

    #[func]
    fn register_for_index(&self, path: Str) -> Result<Value, HintedString> {
        Ok(MetadataElem::new(
            IndexMarker(VirtualPath::new(path).unwrap_or_else(|_| todo!())).into_value(),
        )
        .into_value())
    }
}

fn relative_from_parent(path: &VirtualPath, base: &VirtualPath) -> EcoString {
    let relative = base
        .parent()
        .map_or_else(|| unreachable!(), |parent| path.relative_from(&parent));

    if relative.is_empty() && path != base {
        ".".into()
    } else {
        relative
    }
}
