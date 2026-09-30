use serde::{Deserialize, Serialize};

use super::scan::Track;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

/// Playlist or folder that is playing, plus a separate "play these next" list.
///
/// `tracks` and `index` are the playback context. `user_queue` is only what
/// Add to queue appended. Those songs play before the context advances, and
/// they are not written into `tracks`.
#[derive(Debug, Clone)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub index: usize,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    order: Vec<usize>,
    order_pos: usize,
    user_queue: Vec<Track>,
    /// User-queue song that was shifted out and is playing now.
    /// `index` stays on the context song that was current when it started.
    active_user: Option<Track>,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            index: 0,
            repeat: RepeatMode::Off,
            shuffle: false,
            order: Vec::new(),
            order_pos: 0,
            user_queue: Vec::new(),
            active_user: None,
        }
    }
}

impl Queue {
    pub fn with_modes(repeat: RepeatMode, shuffle: bool) -> Self {
        Self {
            repeat,
            shuffle,
            ..Self::default()
        }
    }

    pub fn is_idle(&self) -> bool {
        self.active_user.is_none() && self.tracks.is_empty()
    }

    /// Install a new context and drop anything waiting in the user queue.
    pub fn replace(&mut self, tracks: Vec<Track>, start: usize) {
        self.user_queue.clear();
        self.active_user = None;
        self.tracks = tracks;
        self.index = start.min(self.tracks.len().saturating_sub(1));
        if self.tracks.is_empty() {
            self.index = 0;
        }
        self.rebuild_order();
    }

    /// Queue a song to play before the context moves on. Does not change
    /// context tracks or the context index.
    pub fn enqueue(&mut self, track: Track) {
        self.user_queue.push(track);
    }

    pub fn current(&self) -> Option<&Track> {
        if let Some(track) = self.active_user.as_ref() {
            return Some(track);
        }
        self.tracks.get(self.index)
    }

    pub fn play_index(&mut self, index: usize) -> Option<&Track> {
        if index >= self.tracks.len() {
            return None;
        }
        self.user_queue.clear();
        self.active_user = None;
        self.index = index;
        self.rebuild_order();
        self.current()
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
        self.rebuild_order();
    }

    pub fn refresh_tracks(&mut self, fresh: &[Track]) {
        apply_fresh(&mut self.tracks, fresh);
        apply_fresh(&mut self.user_queue, fresh);
        if let Some(active) = self.active_user.as_mut() {
            if let Some(next) = fresh.iter().find(|item| item.path == active.path) {
                *active = next.clone();
            }
        }
    }

    /// Next context index, ignoring the user queue. Repeat one stays put.
    pub fn peek_next_index(&self) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return Some(self.index.min(self.tracks.len() - 1));
        }
        if self.order_pos + 1 < self.order.len() {
            return Some(self.order[self.order_pos + 1]);
        }
        if self.repeat == RepeatMode::All {
            return self.order.first().copied();
        }
        None
    }

    /// What plays next: the current song on repeat one, otherwise the first
    /// user-queue song, otherwise the next context song.
    pub fn peek_next(&self) -> Option<&Track> {
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        if let Some(track) = self.user_queue.first() {
            return Some(track);
        }
        let index = self.peek_next_index()?;
        self.tracks.get(index)
    }

    pub fn advance(&mut self) -> Option<&Track> {
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        if !self.user_queue.is_empty() {
            self.active_user = Some(self.user_queue.remove(0));
            return self.active_user.as_ref();
        }
        self.active_user = None;
        self.advance_context()
    }

    pub fn retreat(&mut self) -> Option<&Track> {
        if self.tracks.is_empty() && self.active_user.is_none() {
            return None;
        }
        // A user-queue song already started is consumed. Step back to the
        // context song we left; the rest of the user queue stays next.
        if self.active_user.take().is_some() {
            return self.tracks.get(self.index);
        }
        if self.tracks.is_empty() {
            return None;
        }
        if self.order_pos > 0 {
            self.order_pos -= 1;
            self.index = self.order[self.order_pos];
        } else if self.repeat == RepeatMode::All {
            self.order_pos = self.order.len().saturating_sub(1);
            self.index = self.order[self.order_pos];
        }
        self.current()
    }

    pub fn jump_to_folder(&mut self, folder: &str) -> Option<&Track> {
        let index = self
            .tracks
            .iter()
            .position(|track| track.folder == folder || track.path.starts_with(folder))?;
        self.play_index(index)
    }

    fn advance_context(&mut self) -> Option<&Track> {
        let next = self.peek_next_index()?;
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        let wrapping = self.order_pos + 1 >= self.order.len();
        self.index = next;
        if wrapping && self.repeat == RepeatMode::All {
            self.rebuild_order();
        } else if self.order_pos + 1 < self.order.len() {
            self.order_pos += 1;
        }
        self.current()
    }

    fn rebuild_order(&mut self) {
        let n = self.tracks.len();
        if n == 0 {
            self.order.clear();
            self.order_pos = 0;
            return;
        }
        self.index = self.index.min(n - 1);
        if self.shuffle && n > 1 {
            let mut rest: Vec<usize> = (0..n).filter(|i| *i != self.index).collect();
            fastrand::shuffle(&mut rest);
            self.order = Vec::with_capacity(n);
            self.order.push(self.index);
            self.order.extend(rest);
            self.order_pos = 0;
        } else {
            self.order = (0..n).collect();
            self.order_pos = self.index;
        }
    }
}

fn apply_fresh(tracks: &mut [Track], fresh: &[Track]) {
    for track in tracks {
        if let Some(next) = fresh.iter().find(|item| item.path == track.path) {
            *track = next.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track {
                path: format!("/t{i}.mp3"),
                title: format!("T{i}"),
                artist: "A".into(),
                album: "B".into(),
                album_artist: "A".into(),
                track: Some(i as u32 + 1),
                disc: Some(1),
                duration_ms: 1000,
                folder: "/".into(),
                replaygain_track: None,
                replaygain_album: None,
            })
            .collect()
    }

    fn one(path: &str) -> Track {
        let mut track = tracks(1).remove(0);
        track.path = path.into();
        track.title = path.into();
        track
    }

    #[test]
    fn next_stops_at_end_without_repeat() {
        let mut queue = Queue::default();
        queue.replace(tracks(2), 0);
        assert_eq!(queue.peek_next_index(), Some(1));
        queue.advance();
        assert_eq!(queue.index, 1);
        assert_eq!(queue.peek_next_index(), None);
    }

    #[test]
    fn repeat_one_stays_on_track() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 1);
        queue.set_repeat(RepeatMode::One);
        assert_eq!(queue.peek_next_index(), Some(1));
        queue.advance();
        assert_eq!(queue.index, 1);
    }

    #[test]
    fn repeat_all_wraps() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 2);
        queue.set_repeat(RepeatMode::All);
        assert_eq!(queue.peek_next_index(), Some(0));
        queue.advance();
        assert_eq!(queue.index, 0);
    }

    #[test]
    fn enqueue_does_not_change_context() {
        let mut queue = Queue::default();
        queue.replace(tracks(2), 0);
        let order = queue.order.clone();
        queue.enqueue(one("/extra.mp3"));
        assert_eq!(queue.tracks.len(), 2);
        assert_eq!(queue.index, 0);
        assert_eq!(queue.order, order);
        assert_eq!(queue.user_queue.len(), 1);
        assert_eq!(
            queue.peek_next().map(|track| track.path.as_str()),
            Some("/extra.mp3")
        );
    }

    #[test]
    fn enqueue_keeps_shuffle_order() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 1);
        queue.set_shuffle(true);
        let before = queue.order.clone();
        queue.enqueue(one("/extra.mp3"));
        assert_eq!(queue.order, before);
        assert_eq!(queue.index, 1);
        assert_eq!(queue.user_queue.len(), 1);
    }

    #[test]
    fn user_queue_plays_then_context_advances() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 0);
        queue.enqueue(one("/q1.mp3"));
        queue.enqueue(one("/q2.mp3"));
        assert_eq!(
            queue.advance().map(|track| track.path.as_str()),
            Some("/q1.mp3")
        );
        assert_eq!(queue.index, 0);
        assert_eq!(
            queue.advance().map(|track| track.path.as_str()),
            Some("/q2.mp3")
        );
        assert_eq!(queue.index, 0);
        assert_eq!(
            queue.advance().map(|track| track.path.as_str()),
            Some("/t1.mp3")
        );
        assert_eq!(queue.index, 1);
        assert!(queue.user_queue.is_empty());
        assert!(queue.active_user.is_none());
    }

    #[test]
    fn replace_clears_user_queue() {
        let mut queue = Queue::default();
        queue.replace(tracks(2), 0);
        queue.enqueue(one("/extra.mp3"));
        queue.advance();
        queue.replace(tracks(2), 1);
        assert!(queue.user_queue.is_empty());
        assert!(queue.active_user.is_none());
        assert_eq!(queue.index, 1);
        assert_eq!(
            queue.current().map(|track| track.path.as_str()),
            Some("/t1.mp3")
        );
    }

    #[test]
    fn repeat_one_does_not_drain_user_queue() {
        let mut queue = Queue::default();
        queue.replace(tracks(2), 0);
        queue.enqueue(one("/extra.mp3"));
        queue.set_repeat(RepeatMode::One);
        assert_eq!(
            queue.advance().map(|track| track.path.as_str()),
            Some("/t0.mp3")
        );
        assert_eq!(queue.user_queue.len(), 1);
        assert_eq!(queue.index, 0);
    }

    #[test]
    fn previous_from_user_track_returns_to_context() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 2);
        queue.enqueue(one("/extra.mp3"));
        queue.advance();
        assert_eq!(
            queue.current().map(|track| track.path.as_str()),
            Some("/extra.mp3")
        );
        queue.retreat();
        assert_eq!(queue.index, 2);
        assert!(queue.active_user.is_none());
        assert_eq!(
            queue.current().map(|track| track.path.as_str()),
            Some("/t2.mp3")
        );
        queue.retreat();
        assert_eq!(queue.index, 1);
    }

    #[test]
    fn previous_walks_back() {
        let mut queue = Queue::default();
        queue.replace(tracks(3), 2);
        queue.retreat();
        assert_eq!(queue.index, 1);
        queue.retreat();
        assert_eq!(queue.index, 0);
        queue.retreat();
        assert_eq!(queue.index, 0);
    }
}
