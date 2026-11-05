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
use crate::typeset::color::Color;
use crate::typeset::images::animation::{Animation, FRAME_BUDGET, Plays};
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

/// How long after the end by kiri's clock an animation is taken as ended.
/// The clock starts when kiri sends the start command, and the terminal
/// starts playing a little later, so the switch to the ended picture may
/// come late, and never early.
pub const END_GRACE: Duration = Duration::from_millis(250);

/// Where an animation is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// On its first frame.
    Stopped,
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
}

/// The state of every animation of the document: none plays by itself, and
/// one plays at a time.
#[derive(Debug)]
pub struct Animator {
    /// The most plays of an animation, as the setting says; 0 for no
    /// limit.
    plays: u32,
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
}

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
            Phase::Stopped => Duration::ZERO,
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
    pub fn new(plays: u32) -> Self {
        Animator {
            plays,
            redraw: false,
            drawn: HashMap::new(),
            members: Vec::new(),
            order: Vec::new(),
            entries: HashMap::new(),
            last: None,
            view: None,
            owed: Vec::new(),
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
        match self.members.iter().find(|member| member.part == *key) {
            Some(member) => member.rows.contains(&row).then(|| member.picture.clone()),
            None => self.entries.contains_key(key).then(|| key.clone()),
        }
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
        self.entries.contains_key(key) || self.members.iter().any(|member| member.part == *key)
    }

    /// The plays of the animation `key` under the setting.
    fn plays_of(&self, key: &ImageKey) -> Plays {
        plays_under(self.entries[key].clip.plays, self.plays)
    }

    /// The animation that plays, if one does.
    pub fn playing(&self) -> Option<&ImageKey> {
        self.order
            .iter()
            .find(|key| matches!(self.entries[*key].phase, Phase::Playing(_)))
    }

    /// Starts the animation `key` from its first frame, as it was not
    /// playing, and stops the one that plays.
    pub fn start(&mut self, key: &ImageKey, now: Instant) -> Vec<Command> {
        if !self.entries.contains_key(key) {
            return Vec::new();
        }
        let mut commands = self.stop_commands(Some(key));
        let plays = self.plays_of(key);
        if let Some(entry) = self.entries.get_mut(key) {
            entry.phase = Phase::Playing(now);
        }
        self.last = Some(key.clone());
        commands.push(Command::Replay(key.clone(), plays));
        self.fan(commands)
    }

    /// Stops the animation that plays, back on its first frame.
    pub fn stop(&mut self) -> Vec<Command> {
        let commands = self.stop_commands(None);
        self.fan(commands)
    }

    /// Stops the animation that plays, unless it is `except`: the command
    /// that puts it back on its first frame.
    fn stop_commands(&mut self, except: Option<&ImageKey>) -> Vec<Command> {
        let Some(key) = self.playing().filter(|key| Some(*key) != except).cloned() else {
            return Vec::new();
        };
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.phase = Phase::Stopped;
        }
        vec![Command::Stop(key)]
    }

    /// Answers a click, `p` or Enter on the animation `key`: it starts, or
    /// stops when it plays; one that ended plays again.
    pub fn press(&mut self, key: &ImageKey, now: Instant) -> (Vec<Command>, Pause) {
        let Some(entry) = self.entries.get(key) else {
            return (Vec::new(), Pause::Nothing);
        };
        match entry.phase {
            Phase::Playing(_) => (self.stop(), Pause::Stopped),
            Phase::Stopped | Phase::Ended => (self.start(key, now), Pause::Played),
        }
    }

    /// Puts every animation back on its first frame, none playing: the
    /// document was read again. The commands for the terminal come with
    /// the next tick, once the layout is shown.
    pub fn reset(&mut self) {
        let mut commands = Vec::new();
        for key in &self.order {
            let entry = self.entries.get_mut(key).expect("an entry for each key");
            if entry.phase != Phase::Stopped {
                entry.phase = Phase::Stopped;
                commands.push(Command::Stop(key.clone()));
            }
        }
        let commands = self.fan(commands);
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
        if changed {
            commands.extend(self.stop());
        }
        commands
    }

    /// The terminal was sent `shown` anew, stopped on its first frame: what
    /// plays starts again from there, and what ended is put back on its
    /// last frame.
    pub fn sent(&mut self, shown: &ImageKey, now: Instant) -> Vec<Command> {
        // A part of a page is sent as a part of its picture, whose other
        // parts the terminal may hold at another frame.
        let member = self.members.iter().find(|member| member.part == *shown);
        let key = member.map_or(shown, |member| &member.picture).clone();
        let Some(entry) = self.entries.get_mut(&key) else {
            return Vec::new();
        };
        let plays = plays_under(entry.clip.plays, self.plays);
        let commands = match entry.phase {
            Phase::Playing(_) => {
                entry.phase = Phase::Playing(now);
                vec![Command::Replay(key, plays)]
            }
            Phase::Ended => vec![Command::Show(key, entry.clip.gaps.len().saturating_sub(1))],
            Phase::Stopped => Vec::new(),
        };
        self.fan(commands)
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
            if !matches!(entry.phase, Phase::Playing(_)) {
                continue;
            }
            let plays = self.plays_of(key);
            let played = entry.played_at(now, plays);
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
        let plays = self.plays_of(key);
        match entry.phase {
            Phase::Stopped => 0,
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
    Stopped,
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
        let mut animator = Animator::new(limit);
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
            animator.start(&key("a.gif"), t0),
            [Command::Replay(key("a.gif"), times(3))]
        );
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 1);
    }

    #[test]
    fn starting_one_stops_the_one_that_plays() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        assert_eq!(
            animator.start(&key("b.gif"), t0 + ms(50)),
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
                animator.start(&key("a.gif"), t0),
                [Command::Replay(key("a.gif"), expected)],
                "file {file:?}, setting {limit}"
            );
        }
    }

    #[test]
    fn a_press_stops_what_plays_and_starts_what_does_not() {
        let (mut animator, t0) = known(3, Plays::Forever);
        let (commands, did) = animator.press(&key("a.gif"), t0);
        assert_eq!(commands, [Command::Replay(key("a.gif"), times(3))]);
        assert_eq!(did, Pause::Played);
        let (commands, did) = animator.press(&key("a.gif"), t0 + ms(100));
        assert_eq!(commands, [Command::Stop(key("a.gif"))]);
        assert_eq!(did, Pause::Stopped);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
        let (_, did) = animator.press(&key("nothing.gif"), t0);
        assert_eq!(did, Pause::Nothing);
    }

    #[test]
    fn a_stopped_animation_starts_again_from_the_first_frame() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.start(&key("a.gif"), t0);
        animator.stop();
        animator.start(&key("a.gif"), t0 + ms(1000));
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1000)), 0);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(1150)), 1);
    }

    #[test]
    fn a_change_of_the_view_stops_what_plays() {
        let (mut animator, t0) = known(3, Plays::Forever);
        assert_eq!(animator.tick(t0, 1), []);
        animator.start(&key("a.gif"), t0);
        assert_eq!(animator.tick(t0 + ms(10), 1), []);
        assert_eq!(animator.tick(t0 + ms(20), 2), [Command::Stop(key("a.gif"))]);
        assert_eq!(animator.playing(), None);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(150)), 0);
    }

    #[test]
    fn an_animation_ends_after_its_plays_and_rests_on_its_last_frame() {
        let (mut animator, t0) = known(3, times(2));
        animator.start(&key("a.gif"), t0);
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
        animator.start(&key("a.gif"), t0);
        animator.tick(t0 + ms(299), 1);
        assert_eq!(animator.playing(), Some(&key("a.gif")));
        let due = animator.due(t0 + ms(100), false).expect("an end is due");
        assert!(due >= ms(200), "{due:?}");
        assert_eq!(due, ms(200) + END_GRACE);
    }

    #[test]
    fn an_ended_animation_is_not_stopped_by_a_change_of_the_view() {
        let (mut animator, t0) = known(3, times(1));
        animator.start(&key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        assert_eq!(animator.tick(t0 + ms(2000), 2), []);
        assert_eq!(animator.frame(&key("a.gif"), t0 + ms(2000)), 2);
    }

    #[test]
    fn an_ended_animation_plays_again_on_a_press() {
        let (mut animator, t0) = known(3, times(1));
        animator.start(&key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        let at = t0 + ms(1000);
        let (commands, did) = animator.press(&key("a.gif"), at);
        assert_eq!(commands, [Command::Replay(key("a.gif"), times(1))]);
        assert_eq!(did, Pause::Played);
        assert_eq!(animator.frame(&key("a.gif"), at), 0);
    }

    #[test]
    fn a_reset_puts_all_back_on_their_first_frame_ended_ones_too() {
        let (mut animator, t0) = known(3, times(1));
        animator.start(&key("a.gif"), t0);
        animator.tick(t0 + ms(300) + END_GRACE, 1);
        animator.start(&key("b.gif"), t0 + ms(1000));
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
    fn an_image_sent_again_starts_over_where_it_plays_and_ended_ones_rest() {
        let (mut animator, t0) = known(3, times(1));
        animator.start(&key("a.gif"), t0);
        assert_eq!(
            animator.sent(&key("a.gif"), t0 + ms(100)),
            [Command::Replay(key("a.gif"), times(1))]
        );
        animator.tick(t0 + ms(400) + END_GRACE, 1);
        assert_eq!(
            animator.sent(&key("a.gif"), t0 + ms(2000)),
            [Command::Show(key("a.gif"), 2)]
        );
        // One that is stopped is on its first frame as it was sent.
        assert_eq!(animator.sent(&key("b.gif"), t0), []);
    }

    #[test]
    fn what_kiri_plays_shows_the_frame_of_the_time_played() {
        let (mut animator, t0) = known(3, Plays::Forever);
        animator.redraw(true);
        animator.start(&key("a.gif"), t0);
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
        animator.start(&key("a.gif"), t0);
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
        assert!(notes[0].starts_with("a.gif: its frames take "), "{notes:?}");
        assert!(notes[0].ends_with("over the limit of 64 MiB: only the first frame shows"));
        // Another animation first in the part it is in.
        let few: String = (0..7).map(|n| format!("Paragraph {n}\n\n")).collect();
        let text = format!("![a](a.gif)\n\n{few}![b](b.gif)\n");
        let notes = notes_of(&text, &[("a.gif", 2), ("b.gif", 2)]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("b.gif: another animation is ahead of it"),
            "{notes:?}"
        );
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
        let (_, images) = reading_page("x\n", &[("a.gif", 2), ("b.gif", 2)]);
        inputs.images = images;
        let lay = |text: &str| {
            let _ = crate::typeset::layout::layout(&crate::typeset::parse::parse(text), &inputs);
        };
        // Two animations in one part: the second is left still.
        lay("![a](a.gif)\n\n![b](b.gif)\n");
        assert_eq!(inputs.images.notes().len(), 1);
        // The same images, laid out so that the picture plays: no note.
        lay("![b](b.gif)\n");
        assert_eq!(inputs.images.notes().len(), 0);
        // And back again, once.
        lay("![a](a.gif)\n\n![b](b.gif)\n");
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

    #[test]
    fn a_picture_plays_in_every_part_it_is_in_or_in_none_with_another_animation_above_it() {
        let mut cut = false;
        for before in 3..13 {
            let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
            let text = format!("![a](a.gif)\n\n{text}![b](b.gif)\n");
            let (lines, images) = reading_page(&text, &[("a.gif", 2), ("b.gif", 2)]);
            let mut animator = Animator::new(0);
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

    fn page_of(texts: &[&str], pictures: &[&str]) -> Vec<(String, usize, usize)> {
        let text = texts.join("");
        let names: Vec<(&str, usize)> = pictures.iter().map(|name| (*name, 2)).collect();
        let (lines, images) = reading_page(&text, &names);
        let mut animator = Animator::new(0);
        animator.know(clips(&lines, &images));
        parts_of_pictures(&lines, &images, &animator)
    }

    fn paragraphs(count: usize) -> String {
        (0..count).map(|n| format!("Paragraph {n}\n\n")).collect()
    }

    #[test]
    fn a_picture_that_cannot_play_does_not_stop_one_that_can() {
        // a in part 0, b across the cut of parts 0 and 1, c wholly in part 1.
        let mut seen = false;
        for before in 5..10 {
            let (a, b, c) = ("![a](a.gif)\n\n", "![b](b.gif)\n\n", "![c](c.gif)\n");
            let text = [a, &paragraphs(before), b, &paragraphs(1), c];
            let page = page_of(&text, &["a.gif", "b.gif", "c.gif"]);
            let by = |name: &str| page.iter().find(|p| p.0.ends_with(name)).unwrap().clone();
            if by("b.gif").1 == 2 && by("c.gif").1 == 1 {
                seen = true;
                // b is not first in part 0, so it stays still; c is first in
                // part 1 once b is out of it, and plays.
                assert_eq!(by("b.gif").2, 0, "{page:?} after {before}");
                assert_eq!(by("c.gif").2, 1, "{page:?} after {before}");
                assert_eq!(by("a.gif").2, 1, "{page:?} after {before}");
            }
        }
        assert!(seen, "b is across the cut and c wholly in part 1 for some");
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
        let (_, images) = reading_page("x\n", &[("a.gif", 2), ("b.gif", 40)]);
        let first = "![a](a.gif)\n\n![b](b.gif)\n";
        let second = format!("![a](a.gif)\n\n{}![b](b.gif)\n", paragraphs(4));
        let inputs_a = inputs(images);
        let behind = lay(&inputs_a, first);
        assert!(behind[0].contains("ahead of it"), "{behind:?}");
        // The same images laid out again where b is over the budget: the
        // note says that, as a fresh layout of it does.
        let again = lay(&inputs_a, &second);
        let (_, fresh_images) = reading_page("x\n", &[("a.gif", 2), ("b.gif", 40)]);
        let fresh = lay(&inputs(fresh_images), &second);
        assert_eq!(again, fresh);
    }

    #[test]
    fn only_the_picture_that_stays_still_is_noted() {
        let (a, b, c) = ("![a](a.gif)\n\n", "![b](b.gif)\n\n", "![c](c.gif)\n");
        let text = [a, &paragraphs(6), b, &paragraphs(1), c].concat();
        let notes = notes_of(&text, &[("a.gif", 2), ("b.gif", 2), ("c.gif", 2)]);
        // b is behind a in the part they share; c plays and says nothing.
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("b.gif: another animation is ahead of it"),
            "{notes:?}"
        );
    }

    #[test]
    fn dropping_one_picture_across_the_cut_lets_the_next_one_across_the_next_cut_play() {
        // x in part 0, y across parts 0 and 1, z across parts 1 and 2.
        let mut seen = false;
        for first in 4..10 {
            for second in 6..16 {
                let (x, y, z) = ("![x](x.gif)\n\n", "![y](y.gif)\n\n", "![z](z.gif)\n");
                let text = [x, &paragraphs(first), y, &paragraphs(second), z];
                let page = page_of(&text, &["x.gif", "y.gif", "z.gif"]);
                let by = |name: &str| page.iter().find(|p| p.0.ends_with(name)).unwrap().clone();
                if by("y.gif").1 == 2 && by("z.gif").1 == 2 {
                    seen = true;
                    assert_eq!(by("y.gif").2, 0, "{page:?} after {first}, {second}");
                    assert_eq!(by("z.gif").2, 2, "{page:?} after {first}, {second}");
                }
            }
        }
        assert!(seen, "y and z are across the cuts for some");
    }

    #[test]
    fn a_long_picture_plays_in_every_part_it_is_in_or_in_none_when_a_short_last_part_would_fit() {
        let mut cut = false;
        for before in 8..18 {
            let text: String = (0..before).map(|n| format!("Paragraph {n}\n\n")).collect();
            // Over the budget in a part of 30 rows, under it in a short one.
            let (lines, images) = reading_page(&format!("{text}![a](a.gif)\n"), &[("a.gif", 40)]);
            let mut animator = Animator::new(0);
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
            let mut animator = Animator::new(0);
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
