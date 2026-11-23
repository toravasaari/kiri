//! When which frame of an animated image shows: the state of each animation
//! of the document, as the pager decides it, on the clock it is given.
//!
//! The terminal may play the frames itself (kitty), and then this tells
//! when to start and stop it, as [`Command`]s; or kiri sends each frame in
//! its time (Ghostty), and then [`Animator::frame`] tells which it is. The
//! state is the same either way, and does no I/O.
//!
//! The [`Animator`] holds the state of each animation: stopped on its first
//! frame, playing, or ended on its last. Nothing plays by itself and one plays
//! at a time. It gives the `Command`s that tell a terminal that plays frames
//! itself to replay or stop, and `frame` tells which frame kiri shows when it
//! plays them itself.
//!
//! In reading mode the animation is the picture, not a part of the page:
//! `Scan` and `Member` say which parts show it and on which rows, so that a
//! picture across the cut of two parts is one animation and one Tab stop. The
//! commands name the picture, and the encoder tells every part it has sent
//! with the picture's frames.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::present::encoder::Command;
use crate::session::status::Pill;
use crate::typeset::color::Color;
use crate::typeset::images::animation::{Animation, FRAME_BUDGET, Plays};
use crate::typeset::images::marks::{State, WAIT_STEPS};
use crate::typeset::images::size::{Frame, Placement, Shows};
use crate::typeset::images::{ImageKey, Images, Source};
use crate::typeset::style::{Embed, ImageRow, Line};
use std::rc::Rc;

/// What the state needs of an animation: its frames' gaps and how often it
/// plays.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub gaps: Vec<Duration>,
    pub plays: Plays,
    /// What the status line calls it: its alt text, or else its file's
    /// name without the extension.
    pub name: String,
}

impl Clip {
    /// The clip of `animation`, the picture `key`, which is called `alt`
    /// when the document gave it alt text.
    pub fn of(animation: &Animation, key: &ImageKey, alt: Option<String>) -> Self {
        Clip {
            gaps: animation.frames.iter().map(|frame| frame.gap).collect(),
            plays: animation.plays,
            name: alt.unwrap_or_else(|| stem_of(key)),
        }
    }
}

/// What the status line calls the image `key` when it has no alt text: its
/// file's name without the extension.
pub fn stem_of(key: &ImageKey) -> String {
    let name = name_of(key);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name,
    }
}

/// What the status line calls the image `key`: its file's name.
pub fn name_of(key: &ImageKey) -> String {
    match key {
        ImageKey::Url { url, .. } => {
            let path = url.split(['?', '#']).next().unwrap_or(url);
            path.rsplit('/').next().unwrap_or(path).to_owned()
        }
        other => other.to_string(),
    }
}

/// A part of a page of reading mode that shows a picture, on these rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub part: ImageKey,
    pub picture: ImageKey,
    pub rows: std::ops::Range<u16>,
}

/// What a layout holds of animations: the pictures, and for those in a
/// page the parts that show each, which are one animation.
#[derive(Debug, Default)]
pub struct Scan {
    pub clips: Vec<(ImageKey, Clip)>,
    pub members: Vec<Member>,
}

impl From<Vec<(ImageKey, Clip)>> for Scan {
    fn from(clips: Vec<(ImageKey, Clip)>) -> Self {
        Scan {
            clips,
            members: Vec::new(),
        }
    }
}

/// How long after the end by kiri's clock an animation is taken as ended.
/// The clock starts when kiri sends the start command, and the terminal
/// starts playing a little later, so the switch to the ended picture may
/// come late, and never early.
pub const END_GRACE: Duration = Duration::from_millis(250);

/// How often the mark that says the frames are awaited turns a step.
pub const WAIT_TICK: Duration = Duration::from_millis(125);

/// How long a picture shows the waiting mark at least, though its frames
/// are ready sooner (a build takes some milliseconds): long enough to see
/// the mark turn.
pub const WAIT_MIN: Duration = Duration::from_millis(500);

/// How animations play: at most how many times, and how long a picture shows
/// the waiting mark at least.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pacing {
    /// The most plays of an animation; 0 for as often as its file says.
    pub plays: u32,
    /// The least time the waiting mark shows.
    pub wait: Duration,
}

impl Pacing {
    /// `plays` plays at most, and the mark shown for [`WAIT_MIN`].
    pub fn new(plays: u32) -> Self {
        Self {
            plays,
            wait: WAIT_MIN,
        }
    }

    /// Three plays at most, and the mark gone as soon as the frames are
    /// ready: what a test of something else wants.
    pub fn quick() -> Self {
        Self {
            plays: 3,
            wait: Duration::ZERO,
        }
    }
}

/// How often the status line's pill shows the progress of what plays.
pub const PILL_TICK: Duration = Duration::from_millis(250);

/// Where an animation is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// On its first frame.
    Stopped,
    /// Started at this time, with its frames being built: on its first
    /// frame until they are ready.
    Waiting(Instant),
    /// Playing since this time.
    Playing(Instant),
    /// Played as often as it is to, and rests on its last frame until the
    /// document is read again.
    Ended,
}

#[derive(Debug)]
struct Entry {
    clip: Clip,
    phase: Phase,
    /// Whether its frames could not be built: it stays on its first frame,
    /// with no button.
    unable: bool,
    /// Whether its frames are ready while it waits out [`WAIT_MIN`].
    ready: bool,
}

/// The state of every animation of the document: none plays by itself, and
/// one plays at a time.
#[derive(Debug)]
pub struct Animator {
    /// The most plays of an animation, as the setting says; 0 for no
    /// limit.
    plays: u32,
    /// The least time the waiting mark shows.
    wait: Duration,
    /// Whether kiri plays the frames itself, which then need no commands.
    redraw: bool,
    /// The animations of the layout, in their order.
    order: Vec<ImageKey>,
    /// The frames last drawn, for what kiri plays itself.
    drawn: HashMap<ImageKey, usize>,
    entries: HashMap<ImageKey, Entry>,
    /// The parts of a page that show a picture, in the order of the layout.
    members: Vec<Member>,
    /// The animation that plays or played last, which the status line
    /// shows.
    last: Option<ImageKey>,
    /// What the view was at the last tick: when it changes, what plays
    /// stops.
    view: Option<u64>,
    /// Commands the next tick tells: those of a reset, or of the focus
    /// coming back.
    owed: Vec<Command>,
    /// The animation that played when the focus went, under tmux, until it
    /// comes back.
    away: Option<ImageKey>,
    /// The states last drawn, to tell when a picture needs drawing again.
    drawn_marks: HashMap<ImageKey, State>,
    /// What the pill showed when it was last drawn.
    drawn_pill: Option<PillShown>,
    /// The pictures to have the frames of built, and the builds to stop,
    /// which the loop takes.
    builds: Vec<ImageKey>,
    cancels: Vec<ImageKey>,
}

/// What the pill shows, as far as it is drawn again for it: the animation,
/// its state, the tenths of a second and the cells of the bar filled.
type PillShown = (Option<ImageKey>, State, u128, u8);

/// The plays of an animation whose file says `file`, under the setting
/// `limit`: the smaller of the two, where 0 on either side, or a file that
/// loops without end, is no limit on that side.
pub fn plays_under(file: Plays, limit: u32) -> Plays {
    match (file, limit) {
        (file, 0) => file,
        (Plays::Forever, limit) => Plays::Times(limit),
        (Plays::Times(times), limit) => Plays::Times(times.min(limit)),
    }
}

impl Entry {
    fn duration(&self) -> Duration {
        self.clip.gaps.iter().sum()
    }

    /// The time the animation takes to play as often as it is to; `None`
    /// when it does not end.
    fn limit(&self, plays: Plays) -> Option<Duration> {
        match plays {
            Plays::Forever => None,
            Plays::Times(times) => Some(self.duration() * times),
        }
    }

    /// The time played at `now` of the animation that plays as often as
    /// `plays` says.
    fn played_at(&self, now: Instant, plays: Plays) -> Duration {
        match self.phase {
            Phase::Stopped | Phase::Waiting(_) => Duration::ZERO,
            Phase::Playing(since) => now.saturating_duration_since(since),
            Phase::Ended => self.limit(plays).unwrap_or_default(),
        }
    }

    /// The frame shown after `played` of the animation, that plays as
    /// often as `plays` says.
    fn frame_after(&self, played: Duration, plays: Plays) -> usize {
        let last = self.clip.gaps.len().saturating_sub(1);
        let total = self.duration();
        if total.is_zero() {
            return 0;
        }
        if self.limit(plays).is_some_and(|limit| played >= limit) {
            return last;
        }
        let mut at = Duration::from_nanos((played.as_nanos() % total.as_nanos()) as u64);
        for (frame, gap) in self.clip.gaps.iter().enumerate() {
            if at < *gap {
                return frame;
            }
            at -= *gap;
        }
        last
    }

    /// How long from `played` until the frame changes.
    fn until_next_frame(&self, played: Duration) -> Duration {
        let total = self.duration();
        let mut at = Duration::from_nanos((played.as_nanos() % total.as_nanos().max(1)) as u64);
        for gap in &self.clip.gaps {
            if at < *gap {
                return *gap - at;
            }
            at -= *gap;
        }
        total
    }
}

impl Animator {
    /// An animator for animations that play at most `plays` times, 0 for
    /// as often as their files say.
    pub fn new(pacing: Pacing) -> Self {
        Animator {
            plays: pacing.plays,
            wait: pacing.wait,
            redraw: false,
            drawn: HashMap::new(),
            members: Vec::new(),
            order: Vec::new(),
            entries: HashMap::new(),
            last: None,
            view: None,
            owed: Vec::new(),
            away: None,
            drawn_marks: HashMap::new(),
            drawn_pill: None,
            builds: Vec::new(),
            cancels: Vec::new(),
        }
    }

    /// Has the animator tell the frames of what kiri plays itself, or
    /// the commands for a terminal that plays.
    pub fn redraw(&mut self, redraw: bool) {
        self.redraw = redraw;
    }

    /// Whether kiri plays the frames itself.
    pub fn redraws(&self) -> bool {
        self.redraw
    }

    /// The animations the layout shows, replacing those known before. One
    /// that is known again as it was keeps its state.
    pub fn know(&mut self, scan: impl Into<Scan>) {
        let Scan { clips, members } = scan.into();
        self.members = members;
        let mut entries = HashMap::new();
        let mut order = Vec::new();
        for (key, clip) in clips {
            if entries.contains_key(&key) {
                continue;
            }
            let entry = match self.entries.remove(&key) {
                Some(entry) if entry.clip == clip => entry,
                _ => Entry {
                    clip,
                    phase: Phase::Stopped,
                    unable: false,
                    ready: false,
                },
            };
            order.push(key.clone());
            entries.insert(key, entry);
        }
        self.order = order;
        self.entries = entries;
        self.last = self
            .last
            .take()
            .filter(|key| self.entries.contains_key(key));
    }

    /// The animations of the layout, in their order.
    pub fn keys(&self) -> Vec<ImageKey> {
        self.order.clone()
    }

    /// The animation that row `row` of the image `key` shows: `key` itself
    /// for an image, and for a part of a page the picture, when the row is
    /// on it and not on the rest of the part.
    pub fn picture_at(&self, key: &ImageKey, row: u16) -> Option<ImageKey> {
        let mut members = self.members.iter().filter(|member| member.part == *key);
        if members.next().is_none() {
            return self.entries.contains_key(key).then(|| key.clone());
        }
        (self.members.iter())
            .find(|member| member.part == *key && member.rows.contains(&row))
            .map(|member| member.picture.clone())
    }

    /// The images that show the animation `key`: the parts of a page, or
    /// itself.
    pub fn shown_as(&self, key: &ImageKey) -> Vec<ImageKey> {
        let parts: Vec<ImageKey> = (self.members.iter())
            .filter(|member| member.picture == *key)
            .map(|member| member.part.clone())
            .collect();
        match parts.is_empty() {
            true => vec![key.clone()],
            false => parts,
        }
    }

    /// Whether `key` is an animation, or a part of a page that shows one.
    pub fn knows(&self, key: &ImageKey) -> bool {
        self.entries.contains_key(key) || self.members.iter().any(|member| member.part == *key)
    }

    /// The plays of the animation `key` under the setting.
    fn plays_of(&self, key: &ImageKey) -> Plays {
        plays_under(self.entries[key].clip.plays, self.plays)
    }

    /// Where each animation is in its life, which decides what is drawn in
    /// its picture: a play button, its frames, or a replay icon.
    pub fn marks(&self, now: Instant) -> HashMap<ImageKey, State> {
        (self.entries.iter())
            .filter(|(_, entry)| !entry.unable)
            .map(|(key, entry)| {
                let state = match entry.phase {
                    Phase::Stopped => State::Stopped,
                    Phase::Waiting(since) => {
                        let ticks = now.saturating_duration_since(since).as_millis()
                            / WAIT_TICK.as_millis();
                        State::Waiting((ticks % u128::from(WAIT_STEPS)) as u8)
                    }
                    Phase::Playing(_) => State::Playing,
                    Phase::Ended => State::Ended,
                };
                (key.clone(), state)
            })
            .collect()
    }

    /// Whether the states of the animations changed since this was last
    /// asked: the pictures are to be drawn again.
    pub fn marks_changed(&mut self, now: Instant) -> bool {
        let marks = self.marks(now);
        let changed = marks != self.drawn_marks;
        self.drawn_marks = marks;
        changed
    }

    /// The animation that plays or played last, as the status line tells it:
    /// `None` before the first one starts.
    pub fn pill(&self, now: Instant) -> Option<Pill> {
        let key = self.last.as_ref()?;
        let entry = self.entries.get(key)?;
        let total = entry.duration();
        let (state, elapsed) = match entry.phase {
            Phase::Stopped | Phase::Waiting(_) => (State::Stopped, Duration::ZERO),
            Phase::Ended => (State::Ended, total),
            Phase::Playing(since) => {
                let played = now.saturating_duration_since(since);
                let plays = plays_under(entry.clip.plays, self.plays);
                // How far into the round it is; past the last play, while
                // the end is left to come, the round is all played and does
                // not start again.
                let round = match entry.limit(plays) {
                    Some(limit) if played >= limit => total,
                    _ => Duration::from_nanos((played.as_nanos() % total.as_nanos().max(1)) as u64),
                };
                (State::Playing, round)
            }
        };
        Some(Pill {
            name: entry.clip.name.clone(),
            state,
            elapsed,
            total,
        })
    }

    /// What the animation `key` is called, by its alt text or its file.
    pub fn name_of(&self, key: &ImageKey) -> Option<&str> {
        self.entries.get(key).map(|entry| entry.clip.name.as_str())
    }

    /// Takes the pill off the status line: it comes back with the next start.
    pub fn hide_pill(&mut self) {
        self.last = None;
    }

    /// The animation the pill tells of.
    pub fn last(&self) -> Option<&ImageKey> {
        self.last.as_ref()
    }

    /// The pill as it is printed: the round as it was at the last tick of
    /// [`PILL_TICK`] since the start, so that it moves in quarter-second
    /// steps and the line is written four times a second. The end of the last
    /// play is not put off to a tick.
    pub fn pill_drawn(&self, now: Instant) -> Option<Pill> {
        let key = self.last.as_ref()?;
        let entry = self.entries.get(key)?;
        let Phase::Playing(since) = entry.phase else {
            return self.pill(now);
        };
        let played = now.saturating_duration_since(since);
        let plays = plays_under(entry.clip.plays, self.plays);
        if entry.limit(plays).is_some_and(|limit| played >= limit) {
            return self.pill(now);
        }
        let ticks = played.as_millis() / PILL_TICK.as_millis();
        self.pill(since + PILL_TICK * ticks as u32)
    }

    /// Whether what the pill shows changed since this was last asked: the
    /// animation, its state, the seconds, or a step of its bar.
    pub fn pill_changed(&mut self, now: Instant) -> bool {
        let shown = self.pill_drawn(now).map(|pill| {
            let progress = match pill.total.is_zero() {
                true => 0.0,
                false => pill.elapsed.as_secs_f64() / pill.total.as_secs_f64(),
            };
            // What the pill prints: the tenths of a second (whole seconds
            // from a minute on, as it shows them) and the cells of the bar.
            let tenths = match pill.total.as_secs() < 60 {
                true => pill.elapsed.as_millis() / 100,
                false => pill.elapsed.as_secs() as u128 * 10,
            };
            (
                self.last.clone(),
                pill.state,
                tenths,
                (progress * 8.0).round() as u8,
            )
        });
        let changed = shown != self.drawn_pill;
        self.drawn_pill = shown;
        changed
    }

    /// The reader moved the view to a picture to start it: that move is not
    /// one that stops what plays, nor the wait that follows it.
    pub fn view_moved_to_a_picture(&mut self) {
        self.view = None;
    }

    /// The animation that plays, if one does.
    pub fn playing(&self) -> Option<&ImageKey> {
        self.order
            .iter()
            .find(|key| matches!(self.entries[*key].phase, Phase::Playing(_)))
    }

    /// Starts the animation `key`: it waits, on its first frame, for its
    /// frames to be built (the loop takes the request from
    /// [`Animator::take_builds`]), and plays from the moment [`Animator::built`]
    /// says they are ready. The one that plays, or waits, stops.
    pub fn start(&mut self, key: &ImageKey, now: Instant) -> Vec<Command> {
        if !self.entries.contains_key(key) {
            return Vec::new();
        }
        let commands = self.stop_commands(Some(key));
        if let Some(entry) = self.entries.get_mut(key) {
            entry.phase = Phase::Waiting(now);
            entry.ready = false;
        }
        self.last = Some(key.clone());
        self.builds.push(key.clone());
        commands
    }

    /// The builds to stop and to start, since the last call, as commands for
    /// the loop.
    pub fn pending(&mut self) -> Vec<Command> {
        let cancels = self.take_cancels().into_iter().map(Command::Cancel);
        let builds = self.take_builds().into_iter().map(Command::Build);
        cancels.chain(builds).collect()
    }

    /// The pictures whose frames are to be built, since the last call.
    pub fn take_builds(&mut self) -> Vec<ImageKey> {
        std::mem::take(&mut self.builds)
    }

    /// The pictures whose builds are no longer wanted, since the last call.
    pub fn take_cancels(&mut self) -> Vec<ImageKey> {
        std::mem::take(&mut self.cancels)
    }

    /// The frames of `key` are built, or `ok` is false when they could not
    /// be: it plays from `now`, or stays on its first frame with no button.
    /// A build that comes after the wait was given up does nothing.
    pub fn built(&mut self, key: &ImageKey, ok: bool, now: Instant) -> Vec<Command> {
        let plays = self.entries.get(key).map(|_| self.plays_of(key));
        let wait = self.wait;
        let Some(entry) = self.entries.get_mut(key) else {
            return Vec::new();
        };
        let Phase::Waiting(since) = entry.phase else {
            return Vec::new();
        };
        match (ok, plays) {
            (true, Some(plays)) => {
                // Not before the mark has been seen turning.
                if now < since + wait {
                    entry.ready = true;
                    return Vec::new();
                }
                entry.phase = Phase::Playing(now);
                vec![Command::Replay(key.clone(), plays)]
            }
            _ => {
                entry.phase = Phase::Stopped;
                entry.unable = true;
                Vec::new()
            }
        }
    }

    /// Stops the animation that plays or waits, back on its first frame.
    pub fn stop(&mut self) -> Vec<Command> {
        self.stop_commands(None)
    }

    /// Stops the animation that plays or waits, unless it is `except`: the
    /// command that puts one that plays back on its first frame, or the
    /// cancelling of the build of one that waits.
    fn stop_commands(&mut self, except: Option<&ImageKey>) -> Vec<Command> {
        let active = (self.order.iter())
            .find(|key| {
                matches!(
                    self.entries[*key].phase,
                    Phase::Playing(_) | Phase::Waiting(_)
                ) && Some(*key) != except
            })
            .cloned();
        let Some(key) = active else {
            return Vec::new();
        };
        let entry = self.entries.get_mut(&key).expect("an entry for each key");
        let waiting = matches!(entry.phase, Phase::Waiting(_));
        entry.phase = Phase::Stopped;
        match waiting {
            true => {
                self.cancels.push(key);
                Vec::new()
            }
            false => vec![Command::Stop(key)],
        }
    }

    /// Answers a click, `p` or Enter on the animation `key`: it starts, or
    /// stops when it plays or waits; one that ended plays again.
    pub fn press(&mut self, key: &ImageKey, now: Instant) -> (Vec<Command>, Pause) {
        let Some(entry) = self.entries.get(key).filter(|entry| !entry.unable) else {
            return (Vec::new(), Pause::Nothing);
        };
        match entry.phase {
            Phase::Playing(_) | Phase::Waiting(_) => (self.stop(), Pause::Stopped),
            Phase::Stopped | Phase::Ended => (self.start(key, now), Pause::Played),
        }
    }

    /// Puts every animation back on its first frame, none playing or
    /// waiting: the document was read again. The commands for the terminal
    /// come with the next tick, once the layout is shown.
    pub fn reset(&mut self) {
        let mut commands = Vec::new();
        for key in &self.order {
            let entry = self.entries.get_mut(key).expect("an entry for each key");
            entry.unable = false;
            match entry.phase {
                Phase::Stopped => continue,
                Phase::Waiting(_) => self.cancels.push(key.clone()),
                Phase::Playing(_) | Phase::Ended => commands.push(Command::Stop(key.clone())),
            }
            entry.phase = Phase::Stopped;
        }
        self.owed.extend(commands);
    }

    /// The focus went from the pane, which behind tmux hides it: kitty
    /// pauses what plays in a pane that is not shown, while kiri's clock
    /// runs on. Notes the animation that plays, to put it back on its first
    /// frame on the return ([`Animator::back`]). Nothing for what kiri
    /// plays itself.
    pub fn away(&mut self) {
        if self.redraw {
            return;
        }
        if let Some(key) = self.playing().cloned() {
            self.away = Some(key);
        }
    }

    /// The focus came back. The animation that played when it went, or
    /// that kiri's clock ended meanwhile, is stopped on its first frame, as
    /// if the reader had stopped it: kitty held it where it paused, and kiri
    /// does not start it again, since a restart on the return surprised the
    /// reader. The status line shows it stopped with the time at 0, and its
    /// plays start over when it is started again. The command for the
    /// terminal comes with the next tick; a send of the same animation
    /// that the return brings leaves it stopped, as [`Animator::sent`]
    /// plays only what plays.
    pub fn back(&mut self) {
        let Some(key) = self.away.take() else {
            return;
        };
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        if matches!(entry.phase, Phase::Playing(_) | Phase::Ended) {
            entry.phase = Phase::Stopped;
            self.owed.push(Command::Stop(key));
        }
    }

    /// Takes the time `now` and what the view is, and tells what that
    /// stops: an animation that has played as often as it is to ends, and
    /// the one that plays stops when the view changes.
    pub fn tick(&mut self, now: Instant, view: u64) -> Vec<Command> {
        for key in &self.order {
            let plays = plays_under(self.entries[key].clip.plays, self.plays);
            let entry = self.entries.get_mut(key).expect("an entry for each key");
            if let (Phase::Playing(_), Some(limit)) = (entry.phase, entry.limit(plays))
                && entry.played_at(now, plays) >= limit + END_GRACE
            {
                entry.phase = Phase::Ended;
            }
        }
        let changed = self.view.replace(view).is_some_and(|before| before != view);
        let mut commands = std::mem::take(&mut self.owed);
        // Frames that were ready before the mark was seen start now.
        let wait = self.wait;
        for key in &self.order {
            let plays = plays_under(self.entries[key].clip.plays, self.plays);
            let entry = self.entries.get_mut(key).expect("an entry for each key");
            if let Phase::Waiting(since) = entry.phase
                && entry.ready
                && now >= since + wait
                && !changed
            {
                entry.phase = Phase::Playing(now);
                commands.push(Command::Replay(key.clone(), plays));
            }
        }
        if changed {
            commands.extend(self.stop());
        }
        commands
    }

    /// The terminal was sent the picture `key` with its frames, stopped on
    /// its first frame: it plays from there, if it is to.
    pub fn sent(&mut self, key: &ImageKey, now: Instant) -> Vec<Command> {
        let Some(entry) = self.entries.get_mut(key) else {
            return Vec::new();
        };
        let plays = plays_under(entry.clip.plays, self.plays);
        match entry.phase {
            Phase::Playing(_) => {
                entry.phase = Phase::Playing(now);
                vec![Command::Replay(key.clone(), plays)]
            }
            Phase::Ended | Phase::Stopped | Phase::Waiting(_) => Vec::new(),
        }
    }

    /// How long from `now` until something changes of itself: an animation
    /// ends, or, with `redraw`, for kiri to play, shows another frame.
    pub fn due(&self, now: Instant, redraw: bool) -> Option<Duration> {
        let mut soonest: Option<Duration> = None;
        let mut sooner = |wait: Duration| {
            soonest = Some(soonest.map_or(wait, |soonest| soonest.min(wait)));
        };
        for key in &self.order {
            let entry = &self.entries[key];
            // The mark that says the frames are awaited turns.
            if let Phase::Waiting(since) = entry.phase {
                sooner(WAIT_TICK);
                if entry.ready {
                    sooner((since + self.wait).saturating_duration_since(now));
                }
            }
            if !matches!(entry.phase, Phase::Playing(_)) {
                continue;
            }
            let plays = self.plays_of(key);
            let played = entry.played_at(now, plays);
            // The pill shows the progress four times a second.
            sooner(PILL_TICK);
            if let Some(limit) = entry.limit(plays) {
                sooner((limit + END_GRACE).saturating_sub(played));
            }
            if redraw {
                sooner(entry.until_next_frame(played));
            }
        }
        soonest
    }

    /// The frame each animation shows at `now`, those that show a frame
    /// after the first only.
    pub fn frames(&self, now: Instant) -> HashMap<ImageKey, usize> {
        (self.order.iter())
            .map(|key| (key.clone(), self.frame(key, now)))
            .filter(|(_, frame)| *frame > 0)
            .collect()
    }

    /// Whether an animation in `in_view` shows another frame at `now` than
    /// the one drawn last; the frames are taken as drawn then.
    pub fn changed(&mut self, now: Instant, in_view: &[ImageKey]) -> bool {
        let frames: HashMap<ImageKey, usize> = (in_view.iter())
            .map(|key| (key.clone(), self.frame(key, now)))
            .collect();
        let differs = frames
            .iter()
            .any(|(key, frame)| self.drawn.get(key).copied().unwrap_or(0) != *frame);
        self.drawn.extend(frames);
        differs
    }

    /// The frame `key` shows at `now`, counting from 0.
    pub fn frame(&self, key: &ImageKey, now: Instant) -> usize {
        let Some(entry) = self.entries.get(key) else {
            return 0;
        };
        let plays = self.plays_of(key);
        match entry.phase {
            Phase::Stopped | Phase::Waiting(_) => 0,
            Phase::Playing(_) | Phase::Ended => {
                entry.frame_after(entry.played_at(now, plays), plays)
            }
        }
    }
}

/// The images of `lines` that are animations, with their clips, in the
/// order of the lines, and the parts of pages that show each picture. Only
/// an image that shows as itself plays.
pub fn clips(lines: &[Line], images: &Images) -> Scan {
    let mut scan = Scan::default();
    // Each part of a page is looked at once, and each page's pictures are
    // found once, whatever the number of its lines.
    let mut seen: HashSet<ImageKey> = HashSet::new();
    let mut pages = HashMap::new();
    // The parts of a page of reading mode that hold an animated picture: a
    // picture across the cut of two parts is one animation.
    for line in lines {
        let Some(ImageRow {
            placement,
            page: Some(parts),
            ..
        }) = line.image.as_deref()
        else {
            continue;
        };
        let ImageKey::Page { slice, .. } = placement.key else {
            continue;
        };
        if placement.shows != Shows::Image || !seen.insert(placement.key.clone()) {
            continue;
        }
        let (pictures, unfit) = pages.entry(Rc::as_ptr(parts) as usize).or_insert_with(|| {
            let pictures = parts.animated(images);
            // A picture whose frames would not fit the budget stays still.
            let unfit = parts.unfit(&pictures, FRAME_BUDGET);
            (pictures, unfit)
        });
        let cell = parts.cell_height();
        for (pic, top) in parts.in_part(slice, pictures) {
            let still = unfit
                .iter()
                .any(|(other, _)| (other.x, other.y) == (pic.x, pic.y));
            if still || pic.animation.frames.len() < 2 {
                continue;
            }
            let rows = (top / cell).floor().max(0.0) as u16;
            let end = ((top + pic.height as f32) / cell).ceil() as u16;
            if !scan.clips.iter().any(|(key, _)| *key == pic.key) {
                scan.clips.push((
                    pic.key.clone(),
                    Clip::of(&pic.animation, &pic.key, images.alt(&pic.key)),
                ));
            }
            scan.members.push(Member {
                part: placement.key.clone(),
                picture: pic.key,
                rows: rows..end,
            });
        }
    }
    for key in placements(lines)
        .filter(|placement| placement.shows == Shows::Image)
        .map(|placement| &placement.key)
    {
        if scan.clips.iter().any(|(known, _)| known == key) {
            continue;
        }
        if let Some(Ok(Source::Animated(animation))) = images.get(key)
            && animation.frames.len() > 1
        {
            scan.clips
                .push((key.clone(), Clip::of(animation, key, images.alt(key))));
        }
    }
    scan
}

/// `lines` with the images `keys`, the parts that show the animation
/// `picture`, in a frame of `color` round it, as ones the reader has
/// selected: every placement of them in `lines` gets one, on the rows it is
/// on, and in a part of a page the frame goes round that picture alone.
pub fn frame_selected(lines: &mut [Line], picture: &ImageKey, keys: &[ImageKey], color: Color) {
    let framed = |placement: &Placement, rows: std::ops::Range<u16>| {
        Rc::new(Placement {
            frame: Some(Frame {
                color,
                rows,
                picture: Some(picture.clone()),
            }),
            ..placement.clone()
        })
    };
    for line in lines {
        let block = line.image.as_mut();
        if let Some(row) = block.filter(|row| keys.contains(&row.placement.key.without_overlay())) {
            let rows = 0..row.placement.size.rows;
            row.placement = framed(&row.placement, rows);
        }
        for span in &mut line.spans {
            let embed = span.embed.as_ref();
            let Some(embed) = embed.filter(|e| keys.contains(&e.placement.key.without_overlay()))
            else {
                continue;
            };
            let placement = framed(&embed.placement, embed.ink_rows.clone());
            span.embed = Some(Rc::new(Embed {
                placement,
                ..(**embed).clone()
            }));
        }
    }
}

/// The first of `lines` that shows the animation `key`, if one does.
pub fn first_line_of(lines: &[Line], animator: &Animator, key: &ImageKey) -> Option<usize> {
    (0..lines.len()).find(|at| {
        let shown = pictures_in(&lines[*at..=*at], animator);
        shown.contains(key)
    })
}

/// The animations among the images `lines` place that are in view: for a
/// part of a page, when the picture's rows are.
pub fn keys_in(lines: &[Line], animator: &Animator) -> Vec<ImageKey> {
    pictures_in(lines, animator)
}

fn pictures_in(lines: &[Line], animator: &Animator) -> Vec<ImageKey> {
    let mut keys: Vec<ImageKey> = Vec::new();
    for line in lines {
        let embeds = (line.spans.iter()).filter_map(|span| span.embed.as_ref());
        // A picture in a page of reading mode is only some rows of the part.
        let blocks = line.image.as_ref();
        let blocks = blocks.map(|image| (&image.placement.key, image.row));
        let embeds = embeds.map(|embed| (&embed.placement.key, 0));
        for (key, row) in blocks.into_iter().chain(embeds) {
            let picture = animator.picture_at(key, row);
            if let Some(picture) = picture.filter(|picture| !keys.contains(picture)) {
                keys.push(picture);
            }
        }
    }
    keys
}

/// The placements of the images in `lines`: blocks and embeds.
fn placements(lines: &[Line]) -> impl Iterator<Item = &Placement> {
    lines.iter().flat_map(|line| {
        let embeds = line.spans.iter().filter_map(|span| span.embed.as_ref());
        (line.image.as_ref().map(|image| &*image.placement))
            .into_iter()
            .chain(embeds.map(|embed| &*embed.placement))
    })
}

/// What a toggle did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    Nothing,
    Stopped,
    Played,
}

#[cfg(test)]
mod tests;
