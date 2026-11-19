// "Now Playing" card fed by the Windows media session.
const fmt = (s) => {
  if (!isFinite(s) || s <= 0) return '0:00';
  s = Math.floor(s);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
};

export class NowPlaying {
  constructor(root, { onControl }) {
    this.root = root;
    const $ = (id) => document.getElementById(id);
    this.el = {
      art: $('npArt'), backdrop: $('npBackdrop'), title: $('npTitle'), artist: $('npArtist'),
      album: $('npAlbum'), app: $('npApp'), fill: $('npFill'), knob: $('npKnob'), pos: $('npPos'), dur: $('npDur'),
      artBox: root.querySelector('.np-art'), titleBox: root.querySelector('.np-title'),
    };
    this.pos = 0; this.dur = 0; this.playing = false; this.track = '';
    this.shown = 0; // smoothed progress
    root.classList.add('idle');
    root.querySelectorAll('[data-media]').forEach((b) =>
      b.addEventListener('click', () => {
        const a = b.dataset.media;
        if (a === 'play_pause') this.setPlaying(!this.playing); // optimistic
        onControl(a);
      }));
  }

  setPlaying(p) {
    this.playing = p;
    this.root.classList.toggle('playing', p);
  }

  update(s) {
    if (!s || !s.available) {
      this.root.classList.add('idle'); this.root.classList.remove('has-art');
      this.setPlaying(false);
      this.el.title.textContent = 'Nothing playing';
      this.el.artist.textContent = 'Play music in Spotify, YouTube, Media Player…';
      this.el.album.textContent = ''; this.el.app.textContent = 'Now playing';
      this.dur = 0; this.track = '';
      this.checkMarquee();
      return;
    }
    this.root.classList.remove('idle');
    const changed = s.track_id !== this.track;
    if (changed) {
      this.track = s.track_id;
      for (const k of ['title', 'artist', 'album']) {
        const el = this.el[k];
        el.textContent = s[k] || '';
        el.classList.remove('fade-swap'); void el.offsetWidth; el.classList.add('fade-swap');
      }
      this.el.title.parentElement.title = `${s.title} — ${s.artist}`;
      this.checkMarquee();
      this.shown = s.position;
    }
    if (s.art !== undefined && s.art !== null) {
      if (s.art) {
        this.el.art.src = s.art; this.el.backdrop.src = s.art;
        this.root.classList.add('has-art');
        this.el.artBox.classList.remove('swap'); void this.el.artBox.offsetWidth; this.el.artBox.classList.add('swap');
      } else this.root.classList.remove('has-art');
    }
    this.el.app.textContent = s.app ? `Playing on ${s.app}` : 'Now playing';
    this.setPlaying(s.status === 'playing');
    this.pos = s.position; this.dur = s.duration; this.stamp = performance.now();
    this.el.dur.textContent = fmt(s.duration);
  }

  checkMarquee() {
    const box = this.el.titleBox, span = this.el.title;
    box.classList.remove('scroll');
    requestAnimationFrame(() => {
      const over = span.scrollWidth - box.clientWidth;
      if (over > 4) {
        box.style.setProperty('--shift', `${-over - 40}px`);
        box.style.setProperty('--dur', `${Math.max(7, over / 18)}s`);
        box.classList.add('scroll');
      }
    });
  }

  /** Called every animation frame: interpolates the playhead smoothly. */
  tick(dt) {
    if (!this.dur) { this.el.fill.style.width = '0%'; return; }
    let target = this.pos;
    if (this.playing && this.stamp) target += (performance.now() - this.stamp) / 1000;
    target = Math.min(target, this.dur);
    // glide instead of jumping when a fresh position arrives
    const d = target - this.shown;
    this.shown = Math.abs(d) > 3 ? target : this.shown + d * (1 - Math.exp(-dt * 8));
    const p = (this.shown / this.dur) * 100;
    this.el.fill.style.width = `${p}%`; this.el.knob.style.left = `${p}%`;
    const txt = fmt(this.shown);
    if (this.el.pos.textContent !== txt) this.el.pos.textContent = txt;
  }
}
