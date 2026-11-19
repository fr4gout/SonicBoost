// Themed drop-down that replaces the native <select> popup (which Windows
// always draws in its own style). The real <select> stays in the DOM, hidden,
// as the source of truth: options/value are read from it and `change` is
// dispatched on it, so existing code keeps working unchanged.
const CHECK = '<svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
const CHEVRON = '<svg class="gs-chev" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></svg>';
let openOne = null;

export class GlassSelect {
  constructor(sel) {
    this.sel = sel;
    this.active = -1;
    const wrap = (this.wrap = document.createElement('div'));
    wrap.className = 'gs';
    sel.parentNode.insertBefore(wrap, sel);
    wrap.appendChild(sel);
    sel.classList.add('gs-native');
    sel.tabIndex = -1;
    sel.setAttribute('aria-hidden', 'true');

    const btn = (this.btn = document.createElement('button'));
    btn.type = 'button';
    btn.className = 'gs-btn';
    btn.setAttribute('aria-haspopup', 'listbox');
    btn.setAttribute('aria-expanded', 'false');
    btn.innerHTML = `<span class="gs-label"></span>${CHEVRON}`;
    const label = sel.closest('.field')?.querySelector('label');
    if (label) btn.setAttribute('aria-label', label.textContent.trim());
    wrap.appendChild(btn);

    const list = (this.list = document.createElement('div'));
    list.className = 'gs-list';
    list.setAttribute('role', 'listbox');
    (document.getElementById('app') || document.body).appendChild(list);

    btn.addEventListener('click', () => (this.isOpen ? this.close() : this.open()));
    btn.addEventListener('keydown', (e) => this.onKey(e));
    list.addEventListener('keydown', (e) => this.onKey(e));
    list.addEventListener('pointerdown', (e) => e.preventDefault()); // keep focus on the button
    list.addEventListener('click', (e) => {
      const o = e.target.closest('.gs-opt');
      if (o) this.choose(+o.dataset.i);
    });
    list.addEventListener('pointermove', (e) => {
      const o = e.target.closest('.gs-opt');
      if (o) this.highlight(+o.dataset.i, false);
    });
    new MutationObserver(() => this.refresh()).observe(sel, { childList: true, subtree: true, attributes: true });
    sel.addEventListener('change', () => this.refresh());
    this.refresh();
  }

  destroy() {
    this.close();
    this.list.remove();
  }

  get isOpen() { return this.wrap.classList.contains('open'); }
  get options() { return [...this.sel.options]; }

  refresh() {
    const opts = this.options;
    const cur = this.sel.selectedIndex;
    this.btn.querySelector('.gs-label').textContent = opts[cur]?.textContent || '';
    this.btn.title = opts[cur]?.textContent || '';
    this.list.innerHTML = opts
      .map((o, i) => `<div class="gs-opt${i === cur ? ' selected' : ''}" role="option" aria-selected="${i === cur}" data-i="${i}" title="${o.textContent.replace(/"/g, '&quot;')}"><span>${o.textContent.replace(/</g, '&lt;')}</span>${CHECK}</div>`)
      .join('');
  }

  open() {
    if (openOne && openOne !== this) openOne.close();
    openOne = this;
    this.refresh();
    const r = this.btn.getBoundingClientRect();
    const app = (document.getElementById('app') || document.body).getBoundingClientRect();
    const want = Math.min(260, this.options.length * 34 + 10);
    const below = app.bottom - r.bottom - 10, above = r.top - app.top - 56;
    const up = below < want && above > below;
    const L = this.list;
    L.style.left = `${r.left - app.left}px`;
    L.style.width = `${r.width}px`;
    L.style.maxHeight = `${Math.max(120, Math.min(260, up ? above : below))}px`;
    if (up) { L.style.top = ''; L.style.bottom = `${app.bottom - r.top + 6}px`; }
    else { L.style.bottom = ''; L.style.top = `${r.bottom - app.top + 6}px`; }
    L.classList.toggle('up', up);
    this.wrap.classList.add('open');
    L.classList.add('open');
    this.btn.setAttribute('aria-expanded', 'true');
    this.highlight(Math.max(0, this.sel.selectedIndex), true);
    setTimeout(() => {
      this._outside = (e) => { if (!this.wrap.contains(e.target) && !L.contains(e.target)) this.close(); };
      this._away = () => this.close();
      document.addEventListener('pointerdown', this._outside, true);
      window.addEventListener('resize', this._away);
      this.wrap.closest('.settings-body')?.addEventListener('scroll', this._away, { passive: true });
    });
  }

  close() {
    if (!this.isOpen) return;
    this.wrap.classList.remove('open');
    this.list.classList.remove('open');
    this.btn.setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', this._outside, true);
    window.removeEventListener('resize', this._away);
    this.wrap.closest('.settings-body')?.removeEventListener('scroll', this._away);
    if (openOne === this) openOne = null;
  }

  highlight(i, scroll) {
    const els = this.list.querySelectorAll('.gs-opt');
    if (!els.length) return;
    this.active = Math.max(0, Math.min(els.length - 1, i));
    els.forEach((el, k) => el.classList.toggle('active', k === this.active));
    if (scroll) {
      const el = els[this.active], L = this.list;
      if (el.offsetTop < L.scrollTop) L.scrollTop = el.offsetTop - 4;
      else if (el.offsetTop + el.offsetHeight > L.scrollTop + L.clientHeight) L.scrollTop = el.offsetTop + el.offsetHeight - L.clientHeight + 4;
    }
  }

  choose(i) {
    const changed = i !== this.sel.selectedIndex;
    this.sel.selectedIndex = i;
    this.refresh();
    this.close();
    this.btn.focus();
    if (changed) this.sel.dispatchEvent(new Event('change', { bubbles: true }));
  }

  onKey(e) {
    const n = this.options.length;
    if (!this.isOpen) {
      if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) { e.preventDefault(); this.open(); }
      return;
    }
    if (e.key === 'ArrowDown') { e.preventDefault(); this.highlight(this.active + 1, true); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); this.highlight(this.active - 1, true); }
    else if (e.key === 'Home') { e.preventDefault(); this.highlight(0, true); }
    else if (e.key === 'End') { e.preventDefault(); this.highlight(n - 1, true); }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); this.choose(this.active); }
    else if (e.key === 'Escape' || e.key === 'Tab') { this.close(); }
  }
}
