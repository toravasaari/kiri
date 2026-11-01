//! The renderers a run draws with: each language's external command when it
//! has one, and otherwise the renderer built into kiri, and the threads that
//! draw several diagrams at once.
//!
//! What a renderer is spoken to in, and the renderers built in, are the
//! typesetting engine's (`src/typeset/diagrams/`); finding and running a
//! program is `src/diagrams/find.rs` and `src/diagrams/external.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagrams::{
    BuiltIn, Command, DiagramError, Drawn, External, ExternalSettings, Renderer, Request, external,
    find,
};
use crate::typeset::diagrams::renderer::render_one_by_one;

/// The renderers a run draws with: for each language, its external
/// command when it has one, and otherwise the renderer built into kiri.
/// An external command that fails does not fall back to the built-in
/// renderer, so the user sees that their choice did not work.
#[derive(Debug)]
pub struct Renderers {
    external: External,
}

impl Renderers {
    /// The built-in renderers, and `commands`, the external command for
    /// each language that has one, run as `settings` say.
    pub fn new(commands: BTreeMap<String, Command>, settings: ExternalSettings) -> Self {
        Renderers {
            external: External::new(commands, settings),
        }
    }

    /// The built-in renderers, and the external commands that `configured`
    /// gives or that `search` finds for the languages `no_search` does not
    /// have, run as `settings` say; see `find::choose`.
    pub fn chosen(
        configured: &BTreeMap<String, Vec<String>>,
        no_search: &BTreeSet<String>,
        search: &find::Search,
        settings: ExternalSettings,
    ) -> Self {
        Self::new(find::commands(configured, no_search, search), settings)
    }

    /// Whether programs outside kiri draw any of `languages`.
    pub fn runs_programs_for<'a>(&self, languages: impl IntoIterator<Item = &'a str>) -> bool {
        (languages.into_iter()).any(|language| self.external.draws(language))
    }
}

impl Renderer for Renderers {
    fn draws(&self, language: &str) -> bool {
        self.external.draws(language) || BuiltIn.draws(language)
    }

    fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
        match self.external.draws(request.language) {
            true => self.external.render(request),
            false => BuiltIn.render(request),
        }
    }

    /// Several at once when an external command draws any of them: it
    /// spends most of its time starting up, as `mmdc` does a browser.
    fn render_all(
        &self,
        requests: &[Request],
        done: &(dyn Fn() + Sync),
    ) -> Vec<Result<Drawn, DiagramError>> {
        if (requests.iter()).any(|request| self.external.draws(request.language)) {
            render_in_parallel(self, requests, external::AT_ONCE, done)
        } else {
            // On this thread, whose stack is larger.
            render_one_by_one(self, requests, done)
        }
    }
}

/// `render_all` by `renderer`, drawing up to `at_once` diagrams at a time.
fn render_in_parallel<R: Renderer + Sync + ?Sized>(
    renderer: &R,
    requests: &[Request],
    at_once: usize,
    done: &(dyn Fn() + Sync),
) -> Vec<Result<Drawn, DiagramError>> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<Drawn, DiagramError>>>> =
        requests.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..at_once.max(1).min(requests.len()) {
            // As large a stack as the main thread's, for a built-in
            // renderer among the external ones.
            let thread = std::thread::Builder::new().stack_size(8 << 20);
            let spawned = thread.spawn_scoped(scope, || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(request) = requests.get(i) else {
                        break;
                    };
                    let drawn = renderer.render(request);
                    *results[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(drawn);
                    done();
                }
            });
            // A thread that does not start leaves its share to the others;
            // the last diagrams not drawn by any say so.
            if spawned.is_err() {
                break;
            }
        }
    });
    (results.into_iter())
        .map(|result| {
            let result = result.into_inner().unwrap_or_else(|e| e.into_inner());
            result.unwrap_or(Err(DiagramError::NotDrawn))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::diagrams::Theme;

    #[test]
    fn a_configured_command_draws_in_place_of_the_built_in_renderer() {
        let commands = BTreeMap::from([(
            "mermaid".to_owned(),
            ["/bin/sh", "-c", "exit 4", "{input}", "{output}"]
                .map(String::from)
                .to_vec(),
        )]);
        let settings = || ExternalSettings::new(std::env::temp_dir());
        let renderers = Renderers::chosen(
            &commands,
            &BTreeSet::new(),
            &find::Search::default(),
            settings(),
        );
        assert!(renderers.draws("mermaid") && renderers.draws("Mermaid"));
        // It fails, and the built-in renderer does not step in.
        let drawn = renderers.render(&Request::new(
            "mermaid",
            "flowchart TD\n  A --> B\n",
            Theme::Light,
        ));
        let text = |drawn: Result<Drawn, DiagramError>| drawn.map_err(|error| error.to_string());
        assert_eq!(text(drawn), Err("sh exited with status 4".to_owned()));
        let request = Request::new("mermaid", "flowchart TD\n", Theme::Dark);
        let drawn = renderers.render_all(&[request], &|| {});
        let texts: Vec<_> = drawn.into_iter().map(text).collect();
        assert_eq!(texts, ["sh exited with status 4".to_owned()].map(Err));
        let none = Renderers::new(BTreeMap::new(), settings());
        assert_eq!(none.draws("mermaid"), cfg!(feature = "mermaid"));
        assert!(!none.draws("dot"));
    }

    /// Counts how many diagrams it draws at once.
    #[derive(Default)]
    struct Slow {
        now: AtomicUsize,
        most: AtomicUsize,
    }

    impl Renderer for Slow {
        fn draws(&self, _language: &str) -> bool {
            true
        }

        fn render(&self, request: &Request) -> Result<Drawn, DiagramError> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            self.now.fetch_sub(1, Ordering::SeqCst);
            match request.source {
                "bad" => Err(format!("{}: bad", request.language).into()),
                source => Ok(Drawn::svg(source, request.theme, None)),
            }
        }
    }

    #[test]
    fn draws_up_to_four_diagrams_at_once_and_keeps_their_order() {
        let sources: Vec<String> = (0..10).map(|n| n.to_string()).collect();
        let mut diagrams: Vec<Request> = sources
            .iter()
            .map(|source| Request::new("mermaid", source, Theme::Dark))
            .collect();
        diagrams[3].source = "bad";
        let slow = Slow::default();
        let done = AtomicUsize::new(0);
        let count = || {
            done.fetch_add(1, Ordering::SeqCst);
        };
        let drawn = render_in_parallel(&slow, &diagrams, external::AT_ONCE, &count);
        let svgs: Vec<Result<String, DiagramError>> = (drawn.into_iter())
            .map(|drawn| drawn.map(|drawn| drawn.text()))
            .collect();
        let expected: Vec<Result<String, DiagramError>> = (0..10)
            .map(|n| match n {
                3 => Err("mermaid: bad".into()),
                n => Ok(n.to_string()),
            })
            .collect();
        assert_eq!(svgs, expected);
        // Never more than four, but more than one, however slowly the
        // threads start.
        let most = slow.most.load(Ordering::SeqCst);
        assert!((2..=4).contains(&most), "{most} at once");
        assert_eq!(done.load(Ordering::SeqCst), 10);
        assert!(render_in_parallel(&slow, &[], 4, &count).is_empty());
    }
}
