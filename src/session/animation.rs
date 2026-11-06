//! When which frame of an animated image shows: the state of each animation
//! of the document, as the pager decides it, on the clock it is given.
//!
//! The terminal may play the frames itself (kitty), and then this tells
//! when to start and stop it, as [`Command`]s; or kiri sends each frame in
//! its time (Ghostty), and then [`Animator::frame`] tells which it is. The
//! state is the same either way, and does no I/O.

use std::collections::HashMap;
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

/// An animation Tab stops at: its picture, its name and, for a picture in a
/// page of reading mode, each part of the page that shows it with the rows
/// of the part it is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub key: ImageKey,
    pub name: String,
    pub parts: Vec<(ImageKey, std::ops::Range<u16>)>,
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
    /// Commands the next tick tells: those of a reset.
    owed: Vec<Command>,
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

    /// The animations of the layout, in their order, as Tab stops.
    pub fn stops(&self) -> Vec<Stop> {
        (self.order.iter())
            .map(|key| Stop {
                key: key.clone(),
                name: self.entries[key].clip.name.clone(),
                parts: (self.members.iter())
                    .filter(|member| member.picture == *key)
                    .map(|member| (member.part.clone(), member.rows.clone()))
                    .collect(),
            })
            .collect()
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
    // The parts of a page of reading mode that hold an animated picture: a
    // picture across the cut of two parts is one animation.
    for line in lines {
        let Some(ImageRow {
            placement,
            page: Some(parts),
            ..
        }) = &line.image
        else {
            continue;
        };
        let ImageKey::Page { slice, .. } = placement.key else {
            continue;
        };
        if placement.shows != Shows::Image || (scan.members.iter()).any(|m| m.part == placement.key)
        {
            continue;
        }
        let pictures = parts.animated(images);
        // A picture whose frames would not fit the budget stays still.
        let unfit = parts.unfit(&pictures, FRAME_BUDGET);
        let cell = parts.cell_height();
        for (pic, top) in parts.in_part(slice, &pictures) {
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
        let shown = pictures_in(&lines[*at..=*at], animator, false);
        shown.contains(key)
    })
}

/// The animations among the images `lines` place that are in view: for a
/// part of a page, when the picture's rows are.
pub fn keys_in(lines: &[Line], animator: &Animator) -> Vec<ImageKey> {
    pictures_in(lines, animator, false)
}

/// The animations among the images `lines` place that are no link: a click
/// on a linked image does what a click on a link does.
pub fn plain_keys_in(lines: &[Line], animator: &Animator) -> Vec<ImageKey> {
    pictures_in(lines, animator, true)
}

fn pictures_in(lines: &[Line], animator: &Animator, plain: bool) -> Vec<ImageKey> {
    let mut keys: Vec<ImageKey> = Vec::new();
    for line in lines {
        let embeds = (line.spans.iter()).filter(|span| !plain || span.style.link.is_none());
        let embeds = embeds.filter_map(|span| span.embed.as_ref());
        // A picture in a page of reading mode is only some rows of the part,
        // and a link over it makes it one.
        let blocks = (line.image.as_ref()).filter(|image| !plain || image.links.is_empty());
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
mod tests {
    use super::*;
    use crate::session::bands::Bands;
    use crate::session::links::Links;
    use crate::session::view::View;
    use crate::typeset::images::marks::State;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn key(name: &str) -> ImageKey {
        ImageKey::from(name)
    }

    /// Three frames of 100 ms, 300 ms a play.
    fn clip(name: &str, plays: Plays) -> Clip {
        Clip {
            gaps: vec![ms(100); 3],
            plays,
            name: name.to_owned(),
        }
    }

    fn times(n: u32) -> Plays {
        Plays::Times(n)
    }

    /// An animator with the plays setting `limit` that knows `a.gif` and
    /// `b.gif`, and the time zero.
    fn known(limit: u32, plays: Plays) -> (Animator, Instant) {
        let mut animator = Animator::new(Pacing::new(limit));
        animator.know(vec![
            (key("a.gif"), clip("a.gif", plays)),
            (key("b.gif"), clip("b.gif", plays)),
        ]);
        (animator, Instant::now())
    }

    #[test]
    fn nothing_plays_by_itself() {
        let (mut animator, t0) = known(3, Plays::Forever);
        for at in [0, 1, 1000, 60_000] {
            assert_eq!(animator.tick(t0 + ms(at), 1), []);
        }
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.frames(t0 + ms(5000)), HashMap::new());
        assert_eq!(animator.due(t0, false), None);
    }

    #[test]
    fn a_start_plays_from_the_first_frame() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert_eq!(
            play(&mut animator, &key("a.gif"), t0),
            [Command::Replay(key("a.gif"), times(3))]
        );
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 1);
    }

    /// Starts `key` and has its frames come at `now`, after the time the
    /// mark is shown at least: the commands of both.
    fn play(animator: &mut Animator, key: &ImageKey, now: Instant) -> Vec<Command> {
        let mut commands = animator.start(key, now - WAIT_MIN);
        commands.extend(animator.built(key, true, now));
        commands
    }

    #[test]
    fn a_start_waits_for_the_frames_and_asks_for_them() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert_eq!(animator.start(&key("a.gif"), t0), []);
        assert_eq!(animator.take_builds(), [key("a.gif")]);
        assert_eq!(animator.take_builds(), [], "told once");
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.marks(t0)[&key("a.gif")], State::Waiting(0));
        // Nothing plays while it waits, however long.
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(5000)), 0);
    }

    #[test]
    fn the_play_starts_when_the_frames_are_ready() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        let ready = t0 + ms(700);
        assert_eq!(
            animator.built(&key("a.gif"), true, ready),
            [Command::Replay(key("a.gif"), times(3))]
        );
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        // Counted from when it was ready, not from the press.
        assert_eq!(animator.frame(&key("a.gif"), ready + ms(50)), 0);
        assert_eq!(animator.frame(&key("a.gif"), ready + ms(150)), 1);
        assert_eq!(animator.marks(ready)[&key("a.gif")], State::Playing);
    }

    #[test]
    fn frames_that_could_not_be_built_leave_the_first_frame_and_no_button() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        assert_eq!(animator.built(&key("a.gif"), false, t0 + ms(50)), []);
        assert_eq!(animator.playing(), None);
        // No mark of its own any more, and a press does nothing.
        assert!(!animator.marks(t0 + ms(60)).contains_key(&key("a.gif")));
        assert_eq!(
            animator.press(&key("a.gif"), t0 + ms(100)).1,
            Pause::Nothing
        );
        assert_eq!(animator.take_builds(), [key("a.gif")]);
    }

    #[test]
    fn the_waiting_mark_turns_eight_times_a_second_and_the_pill_four() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        let step = |at: u64| animator.marks(t0 + ms(at))[&key("a.gif")];
        assert_eq!(step(0), State::Waiting(0));
        assert_eq!(step(124), State::Waiting(0));
        assert_eq!(step(125), State::Waiting(1));
        assert_eq!(step(375), State::Waiting(3));
        // A round of eight steps, and round again.
        assert_eq!(step(1000), State::Waiting(0));
        assert_eq!(animator.due(t0 + ms(100), false), Some(WAIT_TICK));
        assert_eq!(WAIT_TICK, ms(125));
        assert_eq!(PILL_TICK, ms(250), "the pill stays at four a second");
        assert!(animator.marks_changed(t0));
        assert!(!animator.marks_changed(t0 + ms(100)));
        assert!(animator.marks_changed(t0 + ms(125)));
    }

    #[test]
    fn frames_that_are_ready_at_once_wait_until_the_mark_can_be_seen_turning() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        // Ready after 10 ms: it does not play yet.
        assert_eq!(animator.built(&key("a.gif"), true, t0 + ms(10)), []);
        assert_eq!(animator.playing(), None);
        assert_eq!(
            animator.marks(t0 + ms(300))[&key("a.gif")],
            State::Waiting(2)
        );
        // The rest of the minimum time is due.
        assert_eq!(
            animator.due(t0 + ms(100), false),
            Some(WAIT_TICK.min(WAIT_MIN - ms(100)))
        );
        assert_eq!(animator.tick(t0 + WAIT_MIN - ms(1), 1), []);
        assert_eq!(
            animator.tick(t0 + WAIT_MIN, 1),
            [Command::Replay(key("a.gif"), times(3))]
        );
        // It plays from then, not from the start or from when it was ready.
        assert_eq!(animator.frame(&key("a.gif"), t0 + WAIT_MIN + ms(50)), 0);
        assert_eq!(animator.frame(&key("a.gif"), t0 + WAIT_MIN + ms(150)), 1);
        // Frames that come late play at once.
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        let late = t0 + WAIT_MIN + ms(300);
        assert_eq!(
            animator.built(&key("a.gif"), true, late),
            [Command::Replay(key("a.gif"), times(3))]
        );
        // Frames that cannot be built give up the wait at once.
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        animator.built(&key("a.gif"), false, t0 + ms(10));
        assert!(!animator.marks(t0 + ms(11)).contains_key(&key("a.gif")));
    }

    #[test]
    fn another_start_a_move_of_the_view_a_second_press_and_a_reset_cancel_the_wait() {
        let (mut animator, t0) = known(3, Plays::Forever);
        // Another start.
        animator.start(&key("a.gif"), t0);
        animator.take_builds();
        animator.start(&key("b.gif"), t0 + ms(10));
        assert_eq!(animator.take_cancels(), [key("a.gif")]);
        assert_eq!(animator.marks(t0)[&key("a.gif")], State::Stopped);
        // The view.
        animator.tick(t0 + ms(20), 1);
        animator.tick(t0 + ms(30), 2);
        assert_eq!(animator.take_cancels(), [key("b.gif")]);
        assert_eq!(animator.marks(t0)[&key("b.gif")], State::Stopped);
        // A second press.
        animator.start(&key("a.gif"), t0 + ms(40));
        animator.press(&key("a.gif"), t0 + ms(50));
        assert_eq!(animator.take_cancels(), [key("a.gif")]);
        // Reload.
        animator.start(&key("b.gif"), t0 + ms(60));
        animator.reset();
        assert_eq!(animator.take_cancels(), [key("b.gif")]);
        // A build that comes after the cancel does nothing.
        assert_eq!(animator.built(&key("b.gif"), true, t0 + ms(70)), []);
        assert_eq!(animator.playing(), None);
    }

    #[test]
    fn starting_one_stops_the_one_that_plays() {
        let (mut animator, t0) = known(3, Plays::Forever);
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(
            play(&mut animator, &key("b.gif"), t0 + ms(50)),
            [
                Command::Stop(key("a.gif")),
                Command::Replay(key("b.gif"), times(3))
            ]
        );
        assert_eq!(animator.playing(), Some(&key("b.gif")));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
    }

    #[test]
    fn the_plays_are_the_smaller_of_the_file_and_the_setting() {
        for (file, limit, expected) in [
            (Plays::Forever, 3, times(3)),
            (times(2), 3, times(2)),
            (times(5), 3, times(3)),
            (Plays::Forever, 0, Plays::Forever),
            (times(5), 0, times(5)),
            (times(1), 0, times(1)),
        ] {
            let (mut animator, t0) = known(limit, file);
            assert_eq!(
                play(&mut animator, &key("a.gif"), t0),
                [Command::Replay(key("a.gif"), expected)],
                "file {file:?}, setting {limit}"
            );
        }
    }

    #[test]
    fn a_press_stops_what_plays_and_starts_what_does_not() {
        let (mut animator, t0) = known(3, Plays::Forever);
        let (commands, did) = animator.press(&key("a.gif"), t0);
        assert_eq!(commands, []);
        assert_eq!(did, Pause::Played);
        assert_eq!(animator.take_builds(), [key("a.gif")]);
        let ready = animator.built(&key("a.gif"), true, t0 + WAIT_MIN);
        assert_eq!(ready, [Command::Replay(key("a.gif"), times(3))]);
        let (commands, did) = animator.press(&key("a.gif"), t0 + WAIT_MIN + ms(100));
        assert_eq!(commands, [Command::Stop(key("a.gif"))]);
        assert_eq!(did, Pause::Stopped);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
        let (_, did) = animator.press(&key("nothing.gif"), t0);
        assert_eq!(did, Pause::Nothing);
    }

    #[test]
    fn a_stopped_animation_starts_again_from_the_first_frame() {
        let (mut animator, t0) = known(3, Plays::Forever);
        play(&mut animator, &key("a.gif"), t0);
        animator.stop();
        play(&mut animator, &key("a.gif"), t0 + ms(1000));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1000)), 0);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1150)), 1);
    }

    #[test]
    fn a_change_of_the_view_stops_what_plays() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert_eq!(animator.tick(t0, 1), []);
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.tick(t0 + ms(10), 1), []);
        assert_eq!(animator.tick(t0 + ms(20), 2), [Command::Stop(key("a.gif"))]);
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
    }

    #[test]
    fn an_animation_ends_after_its_plays_and_rests_on_its_last_frame() {
        let (mut animator, t0) = known(3, times(2));
        play(&mut animator, &key("a.gif"), t0);
        // Two plays of 300 ms; the clock allows the terminal its lag.
        assert_eq!(animator.tick(t0 + ms(600), 1), []);
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(600)), 2);
        assert_eq!(animator.tick(t0 + ms(600) + END_GRACE, 1), []);
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(5000)), 2);
    }

    #[test]
    fn the_end_never_comes_before_the_plays_are_played() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        // One play of 300 ms, and the clock allows the terminal its lag.
        animator.tick(t0 + ms(299), 1);
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        animator.tick(t0 + ms(300) + END_GRACE - ms(1), 1);
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        assert_eq!(animator.playing(), None);
    }

    #[test]
    fn the_pill_does_not_start_a_new_round_before_it_shows_ended() {
        // One play of 300 ms; the end comes a quarter of a second later, by
        // the clock's leave for the terminal's lag.
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        for at in [300, 310, 400, 540] {
            let now = t0 + ms(at);
            animator.tick(now, 1);
            let pill = animator.pill(now).expect("a pill");
            assert_eq!(pill.elapsed, pill.total, "at {at} ms: {pill:?}");
        }
        // A file that plays twice is at the start of its second round at
        // 300 ms, and in the second at 450.
        let (mut animator, t0) = known(3, times(2));
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.pill(t0 + ms(450)).unwrap().elapsed, ms(150));
        // Past the last play it is full, until it is ended.
        assert_eq!(animator.pill(t0 + ms(610)).unwrap().elapsed, ms(300));
        animator.tick(t0 + ms(600) + END_GRACE, 1);
        assert_eq!(animator.pill(t0 + ms(900)).unwrap().state, State::Ended);
    }

    #[test]
    fn the_pill_is_drawn_again_at_each_tick_for_clips_of_any_length() {
        for seconds in [2, 8, 20, 90] {
            let mut animator = Animator::new(Pacing::new(0));
            let clip = Clip {
                gaps: vec![Duration::from_secs(1); seconds],
                plays: Plays::Forever,
                name: "long.gif".to_owned(),
            };
            animator.know(vec![(key("a.gif"), clip)]);
            let t0 = Instant::now();
            play(&mut animator, &key("a.gif"), t0);
            assert!(animator.pill_changed(t0));
            // Every tick of the pill's, for a good while: another picture
            // each time while it prints tenths of a second; from a minute on
            // it prints whole seconds, and is drawn when they change.
            for tick in 1..=24u32 {
                let now = t0 + PILL_TICK * tick;
                let changed = animator.pill_changed(now);
                let at = PILL_TICK * tick;
                match (seconds < 60, tick % 4 == 0) {
                    (true, _) | (false, true) => {
                        assert!(changed, "a clip of {seconds} s at {at:?}")
                    }
                    // Between whole seconds, only a step of the bar.
                    (false, false) => {}
                }
            }
            // And not again for what it has drawn already.
            assert!(!animator.pill_changed(t0 + PILL_TICK * 24 + ms(1)));
        }
    }

    #[test]
    fn the_pill_is_drawn_four_times_a_second_when_polled_ten_times_a_second() {
        // A clip of 59 s, which has no step of the bar in the first 3 s, polled
        // as the loop polls, every 100 ms: one draw for each quarter second
        // since the start, and the first.
        let mut animator = Animator::new(Pacing::new(0));
        let clip = Clip {
            gaps: vec![Duration::from_millis(59_000)],
            plays: Plays::Forever,
            name: "long.gif".to_owned(),
        };
        animator.know(vec![(key("a.gif"), clip)]);
        let t0 = Instant::now();
        play(&mut animator, &key("a.gif"), t0);
        let draws = (0..=30u32)
            .filter(|poll| animator.pill_changed(t0 + ms(100) * *poll))
            .count();
        assert_eq!(draws, 13, "in 3 s");
    }

    #[test]
    fn the_pill_prints_the_round_at_the_last_tick_since_the_start() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        // Between ticks it prints the round at the tick before.
        assert_eq!(animator.pill_drawn(t0 + ms(240)).unwrap().elapsed, ms(0));
        assert_eq!(animator.pill_drawn(t0 + ms(260)).unwrap().elapsed, ms(250));
        // The end of the last play is not put off to the next tick.
        assert_eq!(animator.pill_drawn(t0 + ms(300)).unwrap().elapsed, ms(300));
        assert_eq!(animator.pill_drawn(t0 + ms(310)).unwrap().elapsed, ms(300));
    }

    #[test]
    fn what_plays_is_due_to_show_its_progress_four_times_a_second() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert_eq!(animator.due(t0, false), None);
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.due(t0, false), Some(ms(250)));
        // And the end is due when it comes sooner.
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.due(t0 + ms(500), false), Some(ms(50)));
    }

    #[test]
    fn an_ended_animation_is_not_stopped_by_a_change_of_the_view() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        assert_eq!(animator.tick(t0 + ms(2000), 2), []);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(2000)), 2);
    }

    #[test]
    fn an_ended_animation_plays_again_on_a_press() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        let at = t0 + ms(1000);
        let (commands, did) = animator.press(&key("a.gif"), at);
        assert_eq!(commands, []);
        assert_eq!(did, Pause::Played);
        assert_eq!(animator.take_builds(), [key("a.gif"), key("a.gif")]);
        let ready = animator.built(&key("a.gif"), true, at + WAIT_MIN);
        assert_eq!(ready, [Command::Replay(key("a.gif"), times(1))]);
        assert_eq!(animator.frame(&key("a.gif"), at), 0);
    }

    #[test]
    fn a_reset_puts_all_back_on_their_first_frame_ended_ones_too() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        play(&mut animator, &key("b.gif"), t0 + ms(1000));
        animator.reset();
        let commands = animator.tick(t0 + ms(1050), 1);
        assert_eq!(
            commands,
            [Command::Stop(key("a.gif")), Command::Stop(key("b.gif"))]
        );
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.frames(t0 + ms(1100)), HashMap::new());
    }

    #[test]
    fn an_image_sent_with_its_frames_plays_from_the_first_where_it_is_to_play() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(
            animator.sent(&key("a.gif"), t0 + ms(100)),
            [Command::Replay(key("a.gif"), times(1))]
        );
        animator.tick(t0 + ms(400) + END_GRACE, 1);
        // An ended one is shown as a still image of its last frame, so it
        // is told nothing; nor is one that is stopped.
        assert_eq!(animator.sent(&key("a.gif"), t0 + ms(2000)), []);
        assert_eq!(animator.sent(&key("b.gif"), t0), []);
    }

    #[test]
    fn the_marks_say_where_each_animation_is_in_its_life() {
        let (mut animator, t0) = known(3, times(1));
        let both = |a, b| HashMap::from([(key("a.gif"), a), (key("b.gif"), b)]);
        assert_eq!(animator.marks(t0), both(State::Stopped, State::Stopped));
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.marks(t0), both(State::Playing, State::Stopped));
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        assert_eq!(animator.marks(t0), both(State::Ended, State::Stopped));
        play(&mut animator, &key("b.gif"), t0 + ms(1000));
        assert_eq!(animator.marks(t0), both(State::Ended, State::Playing));
        animator.reset();
        assert_eq!(animator.marks(t0), both(State::Stopped, State::Stopped));
    }

    #[test]
    fn a_change_of_the_marks_is_told_once() {
        let (mut animator, t0) = known(3, times(1));
        assert!(animator.marks_changed(t0), "the first states are new");
        assert!(!animator.marks_changed(t0));
        play(&mut animator, &key("a.gif"), t0);
        assert!(animator.marks_changed(t0));
        assert!(!animator.marks_changed(t0));
    }

    #[test]
    fn what_kiri_plays_shows_the_frame_of_the_time_played() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.redraw(true);
        play(&mut animator, &key("a.gif"), t0);
        assert_eq!(animator.frames(t0 + ms(50)), HashMap::new());
        assert_eq!(
            animator.frames(t0 + ms(150)),
            HashMap::from([(key("a.gif"), 1)])
        );
        assert_eq!(animator.due(t0 + ms(150), true), Some(ms(50)));
    }

    #[test]
    fn new_clips_replace_the_old_and_forgotten_ones_go() {
        let (mut animator, t0) = known(3, Plays::Forever);
        play(&mut animator, &key("a.gif"), t0);
        // Known again as it was: the state stays.
        animator.know(vec![(key("a.gif"), clip("a.gif", Plays::Forever))]);
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        // Changed, or gone: it is stopped on its first frame.
        animator.know(vec![(key("a.gif"), clip("a.gif", times(1)))]);
        assert_eq!(animator.playing(), None);
        animator.know(Vec::new());
        assert!(!animator.knows(&key("a.gif")));
    }

    fn reading_page(text: &str, pictures: &[(&str, usize)]) -> (Vec<Line>, Images) {
        reading_page_of(text, pictures, 200)
    }

    /// As `reading_page`, with frames of 20 × `height` pixels: a row of the
    /// page is 20 pixels.
    fn reading_page_of(text: &str, pictures: &[(&str, usize)], height: u32) -> (Vec<Line>, Images) {
        use crate::typeset::images::animation::Frame;
        let mut images = Images::new();
        for (name, frames) in pictures {
            let frame = Frame {
                image: image::RgbaImage::new(20, height),
                gap: ms(100),
            };
            let animation = Animation {
                frames: vec![frame; *frames],
                plays: Plays::Forever,
                over_budget: None,
            };
            let source = Source::Animated(std::sync::Arc::new(animation));
            images.insert(*name, Ok(source));
        }
        let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
            cell_pixels: Some((10, 20)),
            ..crate::typeset::caps::Capabilities::kitty(80)
        });
        inputs.reading = Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        });
        inputs.images = images;
        let document = crate::typeset::parse::parse(text);
        let lines = crate::typeset::layout::layout(&document, &inputs);
        (lines, inputs.images)
    }

    /// The lines and images of `before` paragraphs and then an animation.
    fn reading_lines(before: usize) -> (Vec<Line>, Images) {
        let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
        reading_page(&format!("{text}![a](a.gif)\n"), &[("a.gif", 2)])
    }

    /// The notes that laying `text` out twice in reading mode leaves in the
    /// images, as text.
    fn notes_of(text: &str, pictures: &[(&str, usize)]) -> Vec<String> {
        let (lines, images) = reading_page(text, pictures);
        drop(lines);
        // The same page again: the layout says it again.
        let (_, again) = reading_page(text, pictures);
        assert_eq!(images.notes().len(), again.notes().len());
        images
            .notes()
            .into_iter()
            .map(|(_, n)| n.to_string())
            .collect()
    }

    #[test]
    fn a_picture_left_on_its_first_frame_is_warned_of_once_and_one_that_plays_is_not() {
        // One animation plays: nothing to say.
        assert_eq!(
            notes_of("![a](a.gif)\n", &[("a.gif", 2)]),
            Vec::<String>::new()
        );
        // Over the budget in a part of the page: 60 frames of a part.
        let text: String = (0..13).map(|n| format!("Paragraph {n}\n\n")).collect();
        let notes = notes_of(&format!("{text}![a](a.gif)\n"), &[("a.gif", 60)]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("a.gif: shows only its first frame: its frames would take "),
            "{notes:?}"
        );
        assert!(notes[0].ends_with("over the limit of 64 MiB"), "{notes:?}");
    }

    #[test]
    fn the_notes_are_those_of_the_last_layout() {
        let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
            cell_pixels: Some((10, 20)),
            ..crate::typeset::caps::Capabilities::kitty(80)
        });
        inputs.reading = Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        });
        let (_, images) = reading_page("x\n", &[("a.gif", 60)]);
        inputs.images = images;
        let lay = |text: &str| {
            let _ = crate::typeset::layout::layout(&crate::typeset::parse::parse(text), &inputs);
        };
        // A long page: its frames do not fit, and the picture is left still.
        let long = format!("{}![a](a.gif)\n", paragraphs(13));
        lay(&long);
        assert_eq!(inputs.images.notes().len(), 1);
        // The same images, laid out so that the picture fits: no note.
        lay("![a](a.gif)\n");
        assert_eq!(inputs.images.notes().len(), 0);
        // And back again, once.
        lay(&long);
        assert_eq!(inputs.images.notes().len(), 1);
    }

    #[test]
    fn laying_the_page_out_again_does_not_say_it_again() {
        let text: String = (0..13).map(|n| format!("Paragraph {n}\n\n")).collect();
        let text = format!("{text}![a](a.gif)\n");
        let (_, images) = reading_page(&text, &[("a.gif", 60)]);
        assert_eq!(images.notes().len(), 1);
        // The same images laid out again, as the pager does at each layout.
        let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
            cell_pixels: Some((10, 20)),
            ..crate::typeset::caps::Capabilities::kitty(80)
        });
        inputs.reading = Some(crate::typeset::reading::engine::Reading {
            pictures: Default::default(),
            fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
        });
        inputs.images = images;
        let document = crate::typeset::parse::parse(&text);
        for _ in 0..3 {
            let _ = crate::typeset::layout::layout(&document, &inputs);
        }
        assert_eq!(inputs.images.notes().len(), 1);
    }

    fn paragraphs(count: usize) -> String {
        (0..count).map(|n| format!("Paragraph {n}\n\n")).collect()
    }

    #[test]
    fn a_note_said_again_by_a_later_layout_has_the_text_of_that_layout() {
        let inputs = |images: Images| {
            let mut inputs = crate::render::Inputs::new(crate::typeset::caps::Capabilities {
                cell_pixels: Some((10, 20)),
                ..crate::typeset::caps::Capabilities::kitty(80)
            });
            inputs.reading = Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
            });
            inputs.images = images;
            inputs
        };
        let lay = |inputs: &crate::render::Inputs, text: &str| {
            let _ = crate::typeset::layout::layout(&crate::typeset::parse::parse(text), inputs);
            inputs
                .images
                .notes()
                .into_iter()
                .map(|(_, n)| n.to_string())
                .collect::<Vec<_>>()
        };
        let (_, images) = reading_page("x\n", &[("b.gif", 60)]);
        let first = format!("{}![b](b.gif)\n", paragraphs(13));
        let second = format!("{}![b](b.gif)\n", paragraphs(17));
        let inputs_a = inputs(images);
        let before = lay(&inputs_a, &first);
        assert_eq!(before.len(), 1, "{before:?}");
        // The same images laid out again on a longer page: the note says what
        // a fresh layout of it does.
        let again = lay(&inputs_a, &second);
        let (_, fresh_images) = reading_page("x\n", &[("b.gif", 60)]);
        let fresh = lay(&inputs(fresh_images), &second);
        assert_eq!(again, fresh);
    }

    #[test]
    fn one_picture_is_one_animation_and_one_stop_wherever_the_parts_of_the_page_cut_it() {
        for before in [0, 5, 11, 12, 14, 25] {
            let (lines, images) = reading_lines(before);
            let mut animator = Animator::new(Pacing::new(0));
            let found = clips(&lines, &images);
            animator.know(found);
            assert_eq!(animator.stops().len(), 1, "after {before} paragraphs");
        }
        // The cut is really there for some of them.
        let parts = |before| {
            let (lines, _) = reading_lines(before);
            let keys: std::collections::HashSet<_> = (lines.iter())
                .filter_map(|line| line.image.as_ref().map(|row| row.placement.key.clone()))
                .collect();
            keys.len()
        };
        assert!((0..30).any(|before| parts(before) > 1));
    }

    #[test]
    fn an_animation_that_does_not_fit_has_no_clip_and_one_that_does_has() {
        // 60 frames in a long page do not fit a part, and 2 do.
        let long = format!("{}![a](a.gif)\n", paragraphs(13));
        let (lines, images) = reading_page(&long, &[("a.gif", 60)]);
        assert!(clips(&lines, &images).clips.is_empty());
        let (lines, images) = reading_page(&long, &[("a.gif", 2)]);
        assert_eq!(clips(&lines, &images).clips.len(), 1);
    }

    #[test]
    fn two_pictures_in_one_part_are_two_animations_on_their_own_rows() {
        let (lines, images) = reading_page(
            "![a](a.gif)\n\n![b](b.gif)\n",
            &[("a.gif", 2), ("b.gif", 3)],
        );
        let scan = clips(&lines, &images);
        assert_eq!(scan.clips.len(), 2);
        let part = scan.members[0].part.clone();
        assert!(scan.members.iter().all(|member| member.part == part));
        let mut animator = Animator::new(Pacing::new(3));
        animator.know(scan);
        let rows: Vec<_> = (animator.stops().iter())
            .map(|stop| stop.parts[0].1.clone())
            .collect();
        assert_ne!(rows[0], rows[1], "each on its own rows");
        // A row of the part is the picture on it, and a row of neither is none.
        let (first, second) = (rows[0].start, rows[1].start);
        assert_eq!(
            animator.picture_at(&part, first),
            Some(animator.stops()[0].key.clone())
        );
        assert_eq!(
            animator.picture_at(&part, second),
            Some(animator.stops()[1].key.clone())
        );
    }

    #[test]
    fn there_is_no_pill_until_an_animation_starts_and_it_stays_after_it_stops() {
        // Without a limit to the plays, so that the rounds go on.
        let mut animator = Animator::new(Pacing::new(0));
        animator.know(vec![(key("a.gif"), clip("a.gif", Plays::Forever))]);
        let t0 = Instant::now();
        assert_eq!(animator.pill(t0), None);
        play(&mut animator, &key("a.gif"), t0);
        let pill = animator.pill(t0 + ms(1500)).expect("a pill");
        assert_eq!(pill.name, "a.gif");
        assert_eq!(pill.state, State::Playing);
        // A round takes 300 ms: 1500 ms in is the start of the sixth.
        assert_eq!(pill.elapsed, ms(0));
        assert_eq!(pill.total, ms(300));
        let pill = animator.pill(t0 + ms(1350)).unwrap();
        assert_eq!(pill.elapsed, ms(150));
        animator.stop();
        let pill = animator.pill(t0 + ms(2000)).expect("it stays");
        assert_eq!((pill.state, pill.elapsed), (State::Stopped, ms(0)));
        assert_eq!(pill.name, "a.gif");
    }

    #[test]
    fn the_pill_is_of_the_animation_that_played_last_and_an_ended_one_is_all_played() {
        let (mut animator, t0) = known(3, times(1));
        play(&mut animator, &key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        let pill = animator.pill(t0 + ms(2000)).unwrap();
        assert_eq!(pill.state, State::Ended);
        assert_eq!((pill.elapsed, pill.total), (ms(300), ms(300)));
        play(&mut animator, &key("b.gif"), t0 + ms(3000));
        assert_eq!(animator.pill(t0 + ms(3100)).unwrap().name, "b.gif");
    }

    #[test]
    fn the_pill_is_drawn_again_when_what_it_shows_changes() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert!(!animator.pill_changed(t0), "no pill, nothing to draw");
        play(&mut animator, &key("a.gif"), t0);
        assert!(animator.pill_changed(t0));
        assert!(!animator.pill_changed(t0 + ms(10)));
        // What it prints moves a quarter of a second at a time.
        assert!(!animator.pill_changed(t0 + ms(100)));
        assert!(animator.pill_changed(t0 + ms(250)));
    }

    #[test]
    fn a_picture_is_named_by_its_alt_text_or_else_its_file_without_the_extension() {
        let (lines, images) = reading_page(
            "![The race horse](horse.gif)\n\n![](jiggy.gif)\n",
            &[("horse.gif", 2), ("jiggy.gif", 2)],
        );
        let scan = clips(&lines, &images);
        let name = |file: &str| {
            let found = scan.clips.iter().find(|(key, _)| key.to_string() == file);
            found.unwrap().1.name.clone()
        };
        assert_eq!(name("horse.gif"), "The race horse");
        assert_eq!(name("jiggy.gif"), "jiggy");
    }

    /// A page of reading mode of `before` paragraphs, an animation of 10
    /// rows and `after` paragraphs, in a view of 20 rows with its top on
    /// line `top`: where Tab (`back` for Shift+Tab) leaves the view, and the
    /// lines of the picture.
    fn tabbed(
        before: usize,
        after: usize,
        top: impl Fn(&[usize]) -> usize,
        back: bool,
    ) -> (View, Vec<usize>) {
        tabbed_of(200, before, after, top, back).0
    }

    /// As `tabbed`, with a picture of `height` pixels, and the number of
    /// Tab stops of the page too.
    fn tabbed_of(
        height: u32,
        before: usize,
        after: usize,
        top: impl Fn(&[usize]) -> usize,
        back: bool,
    ) -> ((View, Vec<usize>), usize) {
        let paragraphs =
            |count: usize| -> String { (0..count).map(|n| format!("Paragraph {n}\n\n")).collect() };
        let text = format!("{}![a](a.gif)\n\n{}", paragraphs(before), paragraphs(after));
        let (lines, images) = reading_page_of(&text, &[("a.gif", 2)], height);
        let mut animator = Animator::new(Pacing::new(0));
        animator.know(clips(&lines, &images));
        let places = vec![None; lines.len()];
        let mut links = Links::default();
        links.know(animator.stops());
        let stops = animator.stops().len();
        // Without a selection, Tab starts from what is on the screen.
        let mut view = View::new(vec![1; lines.len()], 20);
        let mut found = Links::default();
        found.know(animator.stops());
        found.select_next(&lines, &places, 0..lines.len(), false);
        let picture = found
            .target(&lines, &places)
            .expect("the picture")
            .lines()
            .to_vec();
        view.go_to(top(&picture));
        links.select_scrolling(&mut Bands::default(), &mut view, (&lines, &places), back);
        ((view, picture), stops)
    }

    #[test]
    fn tab_shows_a_picture_in_a_page_of_reading_mode_whole_or_in_the_middle() {
        // The picture is 10 rows in a view of 20 rows.
        let (view, p) = tabbed(24, 24, |p| p[0] - 15, false);
        assert_eq!(p.len(), 10);
        assert_eq!(view.visible().end, p[9] + 1, "partly below: bottom edge");
        let (view, p) = tabbed(24, 24, |p| p[0] + 4, false);
        assert_eq!(view.top(), p[0], "partly above: top edge");
        let (view, p) = tabbed(24, 24, |p| p[0] - 3, false);
        assert_eq!(view.top(), p[0] - 3, "shown whole: no move");
        // Wholly below and wholly above: five rows on either side.
        let (view, p) = tabbed(24, 24, |_| 0, false);
        assert_eq!(view.top(), p[0] - 5);
        let (view, p) = tabbed(24, 24, |p| p[0] + 30, true);
        assert_eq!(view.top(), p[0] - 5);
        // At the start of the document there is no room above it.
        let (view, _) = tabbed(2, 24, |p| p[0] + 30, false);
        assert_eq!(view.top(), 0);
    }

    /// The paragraphs before a picture of `height` pixels that put it across
    /// the cut of the page's parts, at row 30, and the first row it is in:
    /// the picture starts on a row from 22 on, below the 20 rows of a view
    /// at the top.
    fn across_the_cut(height: u32) -> usize {
        (0..40)
            .find(|&before| {
                let ((_, p), _) = tabbed_of(height, before, 24, |_| 0, false);
                p[0] >= 22 && p[0] < 30 && p[p.len() - 1] >= 30
            })
            .expect("a page with the picture across the cut")
    }

    #[test]
    fn a_picture_across_the_cut_of_a_page_is_one_stop_and_is_placed_whole() {
        // These tests pin what is right: they were written after it.
        // 25 rows: taller than the view of 20, its top goes to the top.
        let before = across_the_cut(500);
        let ((view, p), stops) = tabbed_of(500, before, 24, |_| 0, false);
        assert_eq!((p.len(), stops), (25, 1));
        assert_eq!(view.top(), p[0]);
        // 15 rows: wholly below the view, so in its middle: 2 rows above.
        let before = across_the_cut(300);
        let ((view, p), stops) = tabbed_of(300, before, 24, |_| 0, false);
        assert_eq!((p.len(), stops), (15, 1));
        assert_eq!(view.top(), p[0] - 2);
    }
}
