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
use crate::session::settings::Animations;
use crate::typeset::color::Color;
use crate::typeset::images::animation::{Animation, FRAME_BUDGET, Plays};
use crate::typeset::images::size::{Frame, Placement, Shows};
use crate::typeset::images::{ImageKey, Images, Source};
use crate::typeset::style::{Embed, ImageRow, Line};
use std::rc::Rc;

pub use crate::typeset::images::animation::START_DELAY;

/// What the state needs of an animation: its frames' gaps and how often it
/// plays.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub gaps: Vec<Duration>,
    pub plays: Plays,
    /// What the status line calls it: its file's name.
    pub name: String,
}

impl Clip {
    /// The clip of `animation`, the picture `key`.
    pub fn of(animation: &Animation, key: &ImageKey) -> Self {
        Clip {
            gaps: animation.frames.iter().map(|frame| frame.gap).collect(),
            plays: animation.plays,
            name: name_of(key),
        }
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

/// Where an animation is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Not playing, and not waiting to: out of view, or not yet in it.
    Idle,
    /// In view, and starts at this time.
    Waiting(Instant),
    /// Playing since this time, on top of what `played` holds.
    Playing(Instant),
    /// Stopped by the reader on the frame `played` tells.
    Paused,
    /// Played as often as it is to, and rests on its last frame.
    Finished,
}

#[derive(Debug)]
struct Entry {
    clip: Clip,
    phase: Phase,
    /// The time played before `Phase::Playing`'s `since`.
    played: Duration,
    /// Whether it has played, so that coming back into view resumes it
    /// at once.
    started: bool,
}

/// The state of every animation of the document.
#[derive(Debug)]
pub struct Animator {
    setting: Animations,
    /// Whether kiri plays the frames itself, which then need no commands.
    redraw: bool,
    /// The animations of the layout, in their order.
    order: Vec<ImageKey>,
    /// The frames last drawn, for what kiri plays itself.
    drawn: HashMap<ImageKey, usize>,
    entries: HashMap<ImageKey, Entry>,
    /// The parts of a page that show a picture, by the part, with the rows
    /// of it the picture is on.
    members: HashMap<ImageKey, (ImageKey, std::ops::Range<u16>)>,
    /// The parts of a page that show each picture, in order.
    parts: HashMap<ImageKey, Vec<ImageKey>>,
}

impl Entry {
    /// The plays of this animation under `setting`: `None` when it does
    /// not play.
    fn plays(&self, setting: Animations) -> Option<Plays> {
        match setting {
            Animations::Never => None,
            Animations::Once => Some(Plays::Times(1)),
            Animations::Always => Some(self.clip.plays),
        }
    }

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

    /// The time played at `now`.
    fn played_at(&self, now: Instant) -> Duration {
        match self.phase {
            Phase::Playing(since) => self.played + now.saturating_duration_since(since),
            _ => self.played,
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
        if plays != Plays::Forever && self.limit(plays).is_some_and(|limit| played >= limit) {
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
    /// An animator for animations that play as `setting` says.
    pub fn new(setting: Animations) -> Self {
        Animator {
            setting,
            redraw: false,
            drawn: HashMap::new(),
            members: HashMap::new(),
            parts: HashMap::new(),
            order: Vec::new(),
            entries: HashMap::new(),
        }
    }

    /// How the animations play.
    pub fn setting(&self) -> Animations {
        self.setting
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
        self.parts.clear();
        for member in &members {
            (self.parts.entry(member.picture.clone()).or_default()).push(member.part.clone());
        }
        self.members = (members.into_iter())
            .map(|member| (member.part, (member.picture, member.rows)))
            .collect();
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
                    phase: Phase::Idle,
                    played: Duration::ZERO,
                    started: false,
                },
            };
            order.push(key.clone());
            entries.insert(key, entry);
        }
        self.order = order;
        self.entries = entries;
    }

    /// The animations of the layout, in their order, as Tab stops.
    pub fn stops(&self) -> Vec<Stop> {
        (self.order.iter())
            .map(|key| Stop {
                key: key.clone(),
                name: self.entries[key].clip.name.clone(),
                parts: (self.members.iter())
                    .filter(|(_, (picture, _))| picture == key)
                    .map(|(part, (_, rows))| (part.clone(), rows.clone()))
                    .collect(),
            })
            .collect()
    }

    /// The animation that row `row` of the image `key` shows: `key` itself
    /// for an image, and for a part of a page the picture, when the row is
    /// on it and not on the rest of the part.
    pub fn picture_at(&self, key: &ImageKey, row: u16) -> Option<ImageKey> {
        match self.members.get(key) {
            Some((picture, rows)) => rows.contains(&row).then(|| picture.clone()),
            None => self.entries.contains_key(key).then(|| key.clone()),
        }
    }

    /// The images that show the animation `key`: the parts of a page, or
    /// itself.
    pub fn shown_as(&self, key: &ImageKey) -> Vec<ImageKey> {
        self.parts
            .get(key)
            .cloned()
            .unwrap_or_else(|| vec![key.clone()])
    }

    /// `commands`, each as many as there are images that show its
    /// animation, so that all the parts of a page are told.
    fn fan(&self, commands: Vec<Command>) -> Vec<Command> {
        (commands.into_iter())
            .flat_map(|command| {
                let shown = self.shown_as(command.key());
                shown.into_iter().map(move |key| command.with_key(key))
            })
            .collect()
    }

    /// Whether `key` is an animation, or a part of a page that shows one.
    pub fn knows(&self, key: &ImageKey) -> bool {
        self.entries.contains_key(key) || self.members.contains_key(key)
    }

    /// Takes the time `now`, with the animations in view, and tells what
    /// that starts and stops.
    pub fn tick(&mut self, now: Instant, in_view: &[ImageKey]) -> Vec<Command> {
        let mut commands = Vec::new();
        let setting = self.setting;
        for key in &self.order {
            let entry = self.entries.get_mut(key).expect("an entry for each key");
            let Some(plays) = entry.plays(setting) else {
                continue;
            };
            if let (Phase::Playing(_), Some(limit)) = (entry.phase, entry.limit(plays))
                && entry.played_at(now) >= limit
            {
                entry.phase = Phase::Finished;
                entry.played = limit;
            }
            let view = in_view.contains(key);
            match (entry.phase, view) {
                (Phase::Idle, true) if entry.started => {
                    entry.phase = Phase::Playing(now);
                    commands.push(Command::Start(key.clone(), plays));
                }
                (Phase::Idle, true) => entry.phase = Phase::Waiting(now + START_DELAY),
                (Phase::Waiting(due), true) if now >= due => {
                    entry.phase = Phase::Playing(now);
                    entry.started = true;
                    commands.push(Command::Start(key.clone(), plays));
                }
                (Phase::Waiting(_), false) => entry.phase = Phase::Idle,
                (Phase::Playing(_), false) => {
                    entry.played = entry.played_at(now);
                    entry.phase = Phase::Idle;
                    commands.push(Command::Stop(key.clone()));
                }
                _ => {}
            }
        }
        self.fan(commands)
    }

    /// The terminal was sent `key` anew, stopped on its first frame: what
    /// plays starts again from there, and what rests is put back on its
    /// frame.
    pub fn sent(&mut self, shown: &ImageKey, now: Instant) -> Vec<Command> {
        // A part of a page is sent as a part of its picture, whose other
        // parts the terminal may hold at another frame.
        let part = self.members.contains_key(shown);
        let key = &self
            .members
            .get(shown)
            .map_or(shown, |(picture, _)| picture)
            .clone();
        let setting = self.setting;
        let Some(entry) = self.entries.get_mut(key) else {
            return Vec::new();
        };
        let Some(plays) = entry.plays(setting) else {
            return Vec::new();
        };
        let commands = match entry.phase {
            Phase::Playing(_) => {
                entry.played = Duration::ZERO;
                entry.phase = Phase::Playing(now);
                // All the parts start over together, from the first frame.
                match part {
                    true => vec![Command::Replay(key.clone(), plays)],
                    false => vec![Command::Start(key.clone(), plays)],
                }
            }
            Phase::Paused | Phase::Finished => match entry.frame_after(entry.played, plays) {
                0 => Vec::new(),
                frame => vec![Command::Show(key.clone(), frame)],
            },
            Phase::Idle if entry.played > Duration::ZERO => {
                // Out of view; the next start resumes from the first frame.
                entry.played = Duration::ZERO;
                Vec::new()
            }
            Phase::Idle | Phase::Waiting(_) => Vec::new(),
        };
        self.fan(commands)
    }

    /// Plays `key` from its first frame.
    pub fn replay(&mut self, key: &ImageKey, now: Instant) -> Vec<Command> {
        let setting = self.setting;
        let Some(entry) = self.entries.get_mut(key) else {
            return Vec::new();
        };
        let Some(plays) = entry.plays(setting) else {
            return Vec::new();
        };
        entry.played = Duration::ZERO;
        entry.phase = Phase::Playing(now);
        entry.started = true;
        self.fan(vec![Command::Replay(key.clone(), plays)])
    }

    /// Pauses what plays of `keys`, or when none does, plays them all.
    pub fn toggle(&mut self, keys: &[ImageKey], now: Instant) -> (Vec<Command>, Pause) {
        let setting = self.setting;
        let known: Vec<&ImageKey> = (keys.iter())
            .filter(|key| {
                (self.entries.get(*key)).is_some_and(|entry| entry.plays(setting).is_some())
            })
            .collect();
        let plays = |animator: &Animator, key: &ImageKey| {
            matches!(
                animator.entries[key].phase,
                Phase::Playing(_) | Phase::Waiting(_)
            )
        };
        if known.is_empty() {
            return (Vec::new(), Pause::Nothing);
        }
        let mut commands = Vec::new();
        if known.iter().any(|key| plays(self, key)) {
            for key in known {
                let entry = self.entries.get_mut(key).expect("a known entry");
                match entry.phase {
                    Phase::Playing(_) => {
                        entry.played = entry.played_at(now);
                        commands.push(Command::Stop(key.clone()));
                    }
                    Phase::Finished => continue,
                    _ => {}
                }
                entry.phase = Phase::Paused;
            }
            return (self.fan(commands), Pause::Paused);
        }
        for key in known {
            let entry = self.entries.get_mut(key).expect("a known entry");
            let plays = entry.plays(setting).expect("a known animation plays");
            let finished = entry.phase == Phase::Finished;
            if finished {
                entry.played = Duration::ZERO;
            }
            entry.phase = Phase::Playing(now);
            entry.started = true;
            commands.push(match finished {
                true => Command::Replay(key.clone(), plays),
                false => Command::Start(key.clone(), plays),
            });
        }
        (self.fan(commands), Pause::Played)
    }

    /// How long from `now` until something changes of itself: an animation
    /// starts, ends, or, with `redraw`, for kiri to play, shows another
    /// frame.
    pub fn due(&self, now: Instant, redraw: bool) -> Option<Duration> {
        let mut soonest: Option<Duration> = None;
        let mut sooner = |wait: Duration| {
            soonest = Some(soonest.map_or(wait, |soonest| soonest.min(wait)));
        };
        for key in &self.order {
            let entry = &self.entries[key];
            let Some(plays) = entry.plays(self.setting) else {
                continue;
            };
            match entry.phase {
                Phase::Waiting(due) => sooner(due.saturating_duration_since(now)),
                Phase::Playing(_) => {
                    let played = entry.played_at(now);
                    if let Some(limit) = entry.limit(plays) {
                        sooner(limit.saturating_sub(played));
                    }
                    if redraw {
                        sooner(entry.until_next_frame(played));
                    }
                }
                _ => {}
            }
        }
        soonest
    }

    /// The frame each animation shows at `now`, those that show a frame
    /// after the first only.
    pub fn frames(&self, now: Instant) -> HashMap<ImageKey, usize> {
        (self.order.iter())
            .map(|key| (key, self.frame(key, now)))
            .filter(|(_, frame)| *frame > 0)
            .flat_map(|(key, frame)| {
                (self.shown_as(key).into_iter()).map(move |shown| (shown, frame))
            })
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
        match entry.plays(self.setting) {
            Some(plays) => entry.frame_after(entry.played_at(now), plays),
            None => 0,
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
        let plays = parts.plays(&pictures, FRAME_BUDGET);
        let Some((pic, top)) = plays.by_part.get(&slice).cloned() else {
            continue;
        };
        let cell = parts.cell_height();
        let rows = (top / cell).floor().max(0.0) as u16;
        let end = ((top + pic.height as f32) / cell).ceil() as u16;
        if !scan.clips.iter().any(|(key, _)| *key == pic.key) {
            scan.clips
                .push((pic.key.clone(), Clip::of(&pic.animation, &pic.key)));
        }
        scan.members.push(Member {
            part: placement.key.clone(),
            picture: pic.key,
            rows: rows..end,
        });
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
            scan.clips.push((key.clone(), Clip::of(animation, key)));
        }
    }
    scan
}

/// `lines` with the images `keys`, the parts that show one animation, in a
/// frame of `color`, as ones the reader has selected: every placement of
/// them in `lines` gets one, on the rows it is on.
pub fn frame_selected(lines: &mut [Line], keys: &[ImageKey], color: Color) {
    let framed = |placement: &Placement, rows: std::ops::Range<u16>| {
        Rc::new(Placement {
            frame: Some(Frame { color, rows }),
            ..placement.clone()
        })
    };
    for line in lines {
        let block = line.image.as_mut();
        if let Some(row) = block.filter(|row| keys.contains(&row.placement.key)) {
            let rows = 0..row.placement.size.rows;
            row.placement = framed(&row.placement, rows);
        }
        for span in &mut line.spans {
            let embed = span.embed.as_ref();
            let Some(embed) = embed.filter(|e| keys.contains(&e.placement.key)) else {
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
    Paused,
    Played,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn key(name: &str) -> ImageKey {
        ImageKey::from(name)
    }

    /// Three frames of 100 ms, 300 ms a play.
    fn clip(plays: Plays) -> Clip {
        Clip {
            gaps: vec![ms(100); 3],
            plays,
            name: "a.gif".to_owned(),
        }
    }

    fn once() -> Plays {
        Plays::Times(1)
    }

    /// An animator of `setting` that knows `a.gif`, and the time zero.
    fn known(setting: Animations, plays: Plays) -> (Animator, Instant) {
        let mut animator = Animator::new(setting);
        animator.know(vec![(key("a.gif"), clip(plays))]);
        (animator, Instant::now())
    }

    fn a() -> [ImageKey; 1] {
        [key("a.gif")]
    }

    #[test]
    fn an_animation_starts_when_its_image_has_been_in_view_for_the_delay() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        assert_eq!(animator.tick(t0, &a()), []);
        assert_eq!(animator.tick(t0 + START_DELAY - ms(1), &a()), []);
        assert_eq!(
            animator.tick(t0 + START_DELAY, &a()),
            [Command::Start(key("a.gif"), once())]
        );
        // Once it plays, it is not started again.
        assert_eq!(animator.tick(t0 + START_DELAY + ms(10), &a()), []);
    }

    #[test]
    fn never_plays_nothing() {
        let (mut animator, t0) = known(Animations::Never, Plays::Forever);
        assert_eq!(animator.tick(t0, &a()), []);
        assert_eq!(animator.tick(t0 + ms(5000), &a()), []);
        assert_eq!(animator.due(t0, false), None);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(5000)), 0);
    }

    #[test]
    fn always_follows_the_files_own_loop_count() {
        let (mut animator, t0) = known(Animations::Always, Plays::Times(3));
        animator.tick(t0, &a());
        assert_eq!(
            animator.tick(t0 + START_DELAY, &a()),
            [Command::Start(key("a.gif"), Plays::Times(3))]
        );
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        animator.tick(t0, &a());
        assert_eq!(
            animator.tick(t0 + START_DELAY, &a()),
            [Command::Start(key("a.gif"), Plays::Forever)]
        );
    }

    #[test]
    fn leaving_the_view_stops_it_and_coming_back_resumes_without_a_delay() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        animator.tick(t0, &a());
        animator.tick(t0 + START_DELAY, &a());
        let left = t0 + START_DELAY + ms(100);
        assert_eq!(animator.tick(left, &[]), [Command::Stop(key("a.gif"))]);
        assert_eq!(animator.tick(left + ms(10), &[]), []);
        assert_eq!(
            animator.tick(left + ms(500), &a()),
            [Command::Start(key("a.gif"), Plays::Forever)]
        );
    }

    #[test]
    fn leaving_before_the_delay_has_passed_starts_the_wait_again() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        assert_eq!(animator.tick(t0 + ms(500), &[]), []);
        assert_eq!(animator.tick(t0 + ms(600), &a()), []);
        assert_eq!(animator.tick(t0 + ms(600) + START_DELAY - ms(1), &a()), []);
        assert_eq!(
            animator.tick(t0 + ms(600) + START_DELAY, &a()),
            [Command::Start(key("a.gif"), once())]
        );
    }

    #[test]
    fn a_finished_animation_rests_whether_in_view_or_not() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        // 300 ms is one play; nothing is sent when it ends.
        assert_eq!(animator.tick(start + ms(300), &a()), []);
        assert_eq!(animator.tick(start + ms(400), &[]), []);
        assert_eq!(animator.tick(start + ms(900), &a()), []);
        assert_eq!(animator.due(start + ms(900), false), None);
    }

    #[test]
    fn only_the_time_in_view_counts_towards_the_end() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        // 200 ms played, then 10 s away: 100 ms are left when it is back.
        animator.tick(start + ms(200), &[]);
        animator.tick(start + ms(10_200), &a());
        assert_eq!(animator.due(start + ms(10_200), false), Some(ms(100)));
        assert_eq!(
            animator.tick(start + ms(10_299), &[]),
            [Command::Stop(key("a.gif"))]
        );
    }

    #[test]
    fn a_file_that_plays_twice_ends_after_two_plays() {
        let (mut animator, t0) = known(Animations::Always, Plays::Times(2));
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        assert_eq!(animator.due(start + ms(500), false), Some(ms(100)));
        assert_eq!(animator.tick(start + ms(600), &a()), []);
        assert_eq!(animator.due(start + ms(600), false), None);
    }

    #[test]
    fn a_replay_plays_from_the_first_frame() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        animator.tick(start + ms(500), &a());
        let again = start + ms(600);
        assert_eq!(
            animator.replay(&key("a.gif"), again),
            [Command::Replay(key("a.gif"), once())]
        );
        assert_eq!(animator.frame(&key("a.gif"), again + ms(150)), 1);
        assert_eq!(animator.due(again + ms(150), false), Some(ms(150)));
        // And it ends 300 ms after.
        assert_eq!(animator.tick(again + ms(300), &a()), []);
        assert_eq!(animator.due(again + ms(300), false), None);
    }

    #[test]
    fn a_pause_stops_it_and_a_second_one_goes_on() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        let (commands, did) = animator.toggle(&a(), start + ms(150));
        assert_eq!(commands, [Command::Stop(key("a.gif"))]);
        assert_eq!(did, Pause::Paused);
        // Paused, it stays on its frame, in view or not.
        assert_eq!(animator.frame(&key("a.gif"), start + ms(5000)), 1);
        assert_eq!(animator.tick(start + ms(3000), &a()), []);
        assert_eq!(animator.tick(start + ms(4000), &[]), []);
        assert_eq!(animator.tick(start + ms(5000), &a()), []);
        let (commands, did) = animator.toggle(&a(), start + ms(5000));
        assert_eq!(commands, [Command::Start(key("a.gif"), Plays::Forever)]);
        assert_eq!(did, Pause::Played);
        assert_eq!(animator.frame(&key("a.gif"), start + ms(5100)), 2);
    }

    #[test]
    fn a_toggle_of_several_pauses_all_when_one_plays_and_plays_all_otherwise() {
        let mut animator = Animator::new(Animations::Always);
        animator.know(vec![
            (key("a.gif"), clip(Plays::Forever)),
            (key("b.gif"), clip(Plays::Forever)),
        ]);
        let t0 = Instant::now();
        let both = [key("a.gif"), key("b.gif")];
        animator.tick(t0, &both);
        animator.tick(t0 + START_DELAY, &both);
        // Pause one, then the toggle of both pauses the other too.
        let at = t0 + START_DELAY + ms(100);
        animator.toggle(&[key("a.gif")], at);
        let (commands, did) = animator.toggle(&both, at);
        assert_eq!(commands, [Command::Stop(key("b.gif"))]);
        assert_eq!(did, Pause::Paused);
        let (commands, did) = animator.toggle(&both, at);
        assert_eq!(did, Pause::Played);
        assert_eq!(
            commands,
            [
                Command::Start(key("a.gif"), Plays::Forever),
                Command::Start(key("b.gif"), Plays::Forever)
            ]
        );
    }

    #[test]
    fn playing_a_finished_animation_starts_it_from_the_first_frame() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        animator.tick(t0 + START_DELAY, &a());
        animator.tick(t0 + START_DELAY + ms(400), &a());
        let (commands, did) = animator.toggle(&a(), t0 + ms(2000));
        assert_eq!(commands, [Command::Replay(key("a.gif"), once())]);
        assert_eq!(did, Pause::Played);
    }

    #[test]
    fn an_image_sent_again_starts_over_where_it_was_playing() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        assert_eq!(animator.sent(&key("a.gif"), t0), []);
        animator.tick(t0, &a());
        animator.tick(t0 + START_DELAY, &a());
        // The terminal has it at the first frame again.
        let at = t0 + START_DELAY + ms(250);
        assert_eq!(
            animator.sent(&key("a.gif"), at),
            [Command::Start(key("a.gif"), Plays::Forever)]
        );
        assert_eq!(animator.frame(&key("a.gif"), at + ms(150)), 1);
    }

    #[test]
    fn what_kiri_plays_shows_the_frame_of_the_time_played() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        let id = key("a.gif");
        assert_eq!(animator.frame(&id, t0), 0);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        assert_eq!(animator.frame(&id, start), 0);
        assert_eq!(animator.frame(&id, start + ms(100)), 1);
        assert_eq!(animator.frame(&id, start + ms(250)), 2);
        assert_eq!(animator.frame(&id, start + ms(300)), 0);
        assert_eq!(animator.due(start + ms(250), true), Some(ms(50)));
    }

    #[test]
    fn a_wait_to_start_is_due_in_the_time_left_of_the_delay() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        assert_eq!(animator.due(t0, true), None);
        animator.tick(t0, &a());
        assert_eq!(
            animator.due(t0 + ms(100), true),
            Some(START_DELAY - ms(100))
        );
        assert_eq!(animator.due(t0 + START_DELAY + ms(5), true), Some(ms(0)));
    }

    #[test]
    fn what_plays_once_rests_on_its_last_frame() {
        let (mut animator, t0) = known(Animations::Once, Plays::Forever);
        animator.tick(t0, &a());
        let start = t0 + START_DELAY;
        animator.tick(start, &a());
        animator.tick(start + ms(400), &a());
        assert_eq!(animator.frame(&key("a.gif"), start + ms(299)), 2);
        assert_eq!(animator.frame(&key("a.gif"), start + ms(10_000)), 2);
        assert_eq!(animator.due(start + ms(400), true), None);
    }

    #[test]
    fn new_clips_replace_the_old_and_forgotten_ones_go() {
        let (mut animator, t0) = known(Animations::Always, Plays::Forever);
        assert!(animator.knows(&key("a.gif")));
        animator.tick(t0, &a());
        animator.tick(t0 + START_DELAY, &a());
        animator.know(vec![(key("b.gif"), clip(Plays::Forever))]);
        assert!(!animator.knows(&key("a.gif")));
        assert!(animator.knows(&key("b.gif")));
        // The same animation known again goes on playing.
        animator.know(vec![(key("b.gif"), clip(Plays::Forever))]);
        assert_eq!(
            animator.tick(t0 + START_DELAY + ms(10), &[key("b.gif")]),
            []
        );
    }

    /// The lines and images of `text` in reading mode on cells of 10 × 20
    /// pixels, with the animations `pictures`: each a name and a number of
    /// frames of 20 × 200 pixels, which is 10 rows.
    fn reading_page(text: &str, pictures: &[(&str, usize)]) -> (Vec<Line>, Images) {
        use crate::typeset::images::animation::Frame;
        let mut images = Images::new();
        for (name, frames) in pictures {
            let frame = Frame {
                image: image::RgbaImage::new(20, 200),
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

    /// For each animated picture of the page of `lines`: how many parts of
    /// the page it is in, and in how many of them it is an animation of the
    /// `animator`. A picture is all of its parts, or none.
    fn parts_of_pictures(
        lines: &[Line],
        images: &Images,
        animator: &Animator,
    ) -> Vec<(String, usize, usize)> {
        let parts = (lines.iter())
            .find_map(|line| line.image.as_ref().and_then(|row| row.page.clone()))
            .expect("a page");
        let pictures = parts.animated(images);
        let count = (lines.iter())
            .filter_map(|line| match line.image.as_ref()?.placement.key {
                ImageKey::Page { slice, .. } => Some(slice + 1),
                _ => None,
            })
            .max()
            .unwrap_or(1);
        let stops = animator.stops();
        (pictures.iter())
            .map(|pic| {
                let spanned = (0..count)
                    .filter(|part| {
                        (parts.in_part(*part, &pictures).iter()).any(|(p, _)| p.y == pic.y)
                    })
                    .count();
                let playing = (stops.iter())
                    .find(|stop| stop.key == pic.key)
                    .map_or(0, |stop| stop.parts.len());
                (pic.key.to_string(), spanned, playing)
            })
            .collect()
    }

    #[test]
    fn a_picture_plays_in_every_part_it_is_in_or_in_none_with_another_animation_above_it() {
        let mut cut = false;
        for before in 3..13 {
            let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
            let text = format!("![a](a.gif)\n\n{text}![b](b.gif)\n");
            let (lines, images) = reading_page(&text, &[("a.gif", 2), ("b.gif", 2)]);
            let mut animator = Animator::new(Animations::Always);
            animator.know(clips(&lines, &images));
            for (name, spanned, playing) in parts_of_pictures(&lines, &images, &animator) {
                cut |= spanned > 1;
                assert!(
                    playing == 0 || playing == spanned,
                    "{name} after {before}: in {spanned} parts, plays in {playing}"
                );
            }
        }
        assert!(cut, "a picture is across the cut for some of them");
    }

    #[test]
    fn a_long_picture_plays_in_every_part_it_is_in_or_in_none_when_a_short_last_part_would_fit() {
        let mut cut = false;
        for before in 8..18 {
            let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
            // Over the budget in a part of 30 rows, under it in a short one.
            let (lines, images) = reading_page(&format!("{text}![a](a.gif)\n"), &[("a.gif", 40)]);
            let mut animator = Animator::new(Animations::Always);
            animator.know(clips(&lines, &images));
            for (name, spanned, playing) in parts_of_pictures(&lines, &images, &animator) {
                cut |= spanned > 1;
                assert!(
                    playing == 0 || playing == spanned,
                    "{name} after {before}: in {spanned} parts, plays in {playing}"
                );
            }
        }
        assert!(cut, "a picture is across the cut for some of them");
    }

    #[test]
    fn one_picture_is_one_animation_and_one_stop_wherever_the_parts_of_the_page_cut_it() {
        for before in [0, 5, 11, 12, 14, 25] {
            let (lines, images) = reading_lines(before);
            let mut animator = Animator::new(Animations::Always);
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
}
