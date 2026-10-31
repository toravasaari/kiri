//! The `Renderer` trait, the renderers built into kiri, and the stand-in
//! for no renderer at all. A renderer that runs a program outside kiri is
//! the platform's, `src/diagrams/external.rs`.

use crate::typeset::diagrams::error::DiagramError;
use crate::typeset::diagrams::language::{self, language};
use crate::typeset::diagrams::request::{Drawn, Request};

/// Draws the diagrams of some languages.
pub trait Renderer {
    /// Whether code blocks in `language`, as the info string gives it, are
    /// drawn as diagrams.
    fn draws(&self, language: &str) -> bool;

    /// The diagram that `request` asks for as an image, or why it could
    /// not be drawn.
    fn render(&self, request: &Request) -> Result<Drawn, DiagramError>;

    /// Each of `requests` drawn as `render` draws it, in order; `done` is
    /// called as each one is finished.
    fn render_all(
        &self,
        requests: &[Request],
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, DiagramError>> {
        render_one_by_one(self, requests, done)
    }
}

/// `render_all` by `renderer`, one diagram after another.
pub(crate) fn render_one_by_one<R: Renderer + ?Sized>(
    renderer: &R,
    requests: &[Request],
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, DiagramError>> {
    (requests.iter())
        .map(|request| {
            let drawn = renderer.render(request);
            done();
            drawn
        })
        .collect()
}

/// The renderers built into kiri, such as merman for Mermaid in a build
/// with the `mermaid` feature.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltIn;

impl BuiltIn {
    /// The renderer built in for `language`, in this build.
    fn of(language: &str) -> Option<language::BuiltInRenderer> {
        self::language(language)?.built_in
    }

    /// What this build has built in to draw `language`, with its version,
    /// such as `merman 0.9.1, Mermaid 12.1.0`; `None` when it has nothing.
    pub fn version(language: &str) -> Option<String> {
        Self::of(language).map(|built_in| (built_in.version)())
    }
}

/// How far below its baseline the diagram `source` in `language` reaches,
/// as a fraction of its image's height, when it has a baseline: the
/// inline formula that [`INLINE_FORMULA`](crate::typeset::diagrams::request::INLINE_FORMULA) names.
pub fn baseline_depth(language: &str, source: &str) -> Option<f64> {
    (BuiltIn::of(language)?.baseline)(source)
}

impl Renderer for BuiltIn {
    fn draws(&self, language: &str) -> bool {
        Self::of(language).is_some()
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        match Self::of(request.language) {
            Some(built_in) => (built_in.render)(request),
            None => Err(DiagramError::NotInBuild(request.language.to_owned())),
        }
    }
}

/// No renderer at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRenderer;

impl Renderer for NoRenderer {
    fn draws(&self, _language: &str) -> bool {
        false
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        Err(DiagramError::NoRenderer(request.language.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::diagrams::language::LANGUAGES;
    use crate::typeset::diagrams::request::Theme;

    #[test]
    fn a_language_that_is_not_drawn_says_so() {
        let request = Request::new("dot", "digraph {}\n", Theme::Light);
        fn text<E: ToString>(drawn: Result<Drawn, E>) -> Result<(), String> {
            drawn.map(|_| ()).map_err(|error| error.to_string())
        }
        assert_eq!(
            text(BuiltIn.render(&request)),
            Err("dot diagrams are not drawn in this build".to_owned())
        );
        assert_eq!(
            text(NoRenderer.render(&request)),
            Err("dot diagrams are not drawn".to_owned())
        );
    }

    #[test]
    fn mermaid_is_built_in_with_its_feature() {
        for language in ["mermaid", "Mermaid"] {
            assert_eq!(BuiltIn.draws(language), cfg!(feature = "mermaid"));
        }
        assert!(!BuiltIn.draws("dot"));
    }

    #[test]
    fn the_built_in_renderer_tells_its_version() {
        assert_eq!(
            BuiltIn::version("mermaid").is_some(),
            cfg!(feature = "mermaid")
        );
        for language in LANGUAGES.iter().map(|language| language.name) {
            let drawn = BuiltIn.draws(language);
            assert_eq!(BuiltIn::version(language).is_some(), drawn, "{language}");
        }
        assert_eq!(BuiltIn::version("dot"), None);
    }

    #[test]
    fn formulas_are_built_in_in_every_build() {
        for language in ["math", "Math"] {
            assert!(BuiltIn.draws(language), "{language}");
        }
        assert!(BuiltIn::version("math").unwrap().starts_with("RaTeX "));
        let drawn = BuiltIn.render(&Request::new("math", "x^2", Theme::Dark));
        assert!(drawn.unwrap().text().starts_with("<svg"));
    }
}
