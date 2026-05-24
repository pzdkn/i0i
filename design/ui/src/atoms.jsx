// Shared UI atoms for the 1O1 prototype.
// All components assume the .crt theme is mounted on a parent.

// Key hint: bracketed shortcut like [K] or [⌘K]
const Key = ({ children, dim }) => (
  <span className={"k" + (dim ? " dim" : "")}>{children}</span>
);

// Label: tiny tracked uppercase
const Label = ({ children, hot, amber, style }) => (
  <span className={"label" + (hot ? " hot" : amber ? " amber" : "")} style={style}>{children}</span>
);

// Small button
const Btn = ({ children, primary, ghost, onClick, style }) => (
  <button
    className={"btn" + (primary ? " primary" : ghost ? " ghost" : "")}
    onClick={onClick}
    style={style}
  >{children}</button>
);

// Chip
const Chip = ({ children, hot, solid, style }) => (
  <span className={"chip" + (hot ? " hot" : solid ? " solid" : "")} style={style}>{children}</span>
);

// Hairline horizontal rule
const HR = ({ style }) => (
  <div style={{ height: 1, background: 'var(--border)', ...style }} />
);

// Dotted divider — ASCII-flavored
const Dotted = ({ char = '·', style }) => (
  <div className="mono-dim" style={{ fontSize: 10, letterSpacing: 1, color: 'var(--fg-4)', overflow: 'hidden', whiteSpace: 'nowrap', ...style }}>
    {char.repeat(400)}
  </div>
);

// Ascii bullet
const Dot = ({ color = 'var(--amber)', size = 7 }) => (
  <span style={{
    display: 'inline-block', width: size, height: size,
    background: color, flexShrink: 0,
  }} />
);

// Gauge bar
const Gauge = ({ value = 0.6, w = 60 }) => (
  <div className="gauge" style={{ width: w }}>
    <i style={{ width: `${Math.round(value * 100)}%` }} />
  </div>
);

// Window title bar (top of each artboard)
const TitleBar = ({ left, center, right, height = 28 }) => (
  <div className="row hair-b" style={{ height, padding: '0 10px', background: 'var(--bg-1)', gap: 12, flexShrink: 0 }}>
    <div className="row gap-8" style={{ flex: '0 0 auto' }}>{left}</div>
    <div className="flex1 row" style={{ justifyContent: 'center', minWidth: 0 }}>{center}</div>
    <div className="row gap-8" style={{ flex: '0 0 auto' }}>{right}</div>
  </div>
);

// Status bar (bottom of each artboard)
const StatusBar = ({ left = [], right = [], height = 22 }) => (
  <div className="row hair-t" style={{ height, padding: '0 10px', background: 'var(--bg-1)', gap: 14, fontSize: 10, color: 'var(--fg-2)', flexShrink: 0 }}>
    {left.map((x, i) => (
      <span key={'l' + i} className="row gap-4">{x}</span>
    ))}
    <div className="flex1" />
    {right.map((x, i) => (
      <span key={'r' + i} className="row gap-4">{x}</span>
    ))}
  </div>
);

// Tree of vault entries (uses window.VAULT_TREE)
const VaultTree = ({ compact, accent = 'var(--amber)' }) => {
  const rowH = compact ? 18 : 20;
  return (
    <div style={{ padding: '6px 0', fontSize: 11 }}>
      {window.VAULT_TREE.map((node, idx) => {
        if (node.kind === 'section') {
          return (
            <div key={idx} className="row hair-b" style={{
              padding: '12px 12px 4px', height: 'auto', alignItems: 'flex-end', borderBottom: '1px solid var(--border)',
              marginTop: idx === 0 ? 0 : 4,
            }}>
              <Label style={{ color: 'var(--fg-3)' }}>{node.label}</Label>
            </div>
          );
        }
        if (node.kind === 'item') {
          return (
            <div key={idx} className="row" style={{ height: rowH, padding: '0 12px', gap: 8, color: node.dim ? 'var(--fg-3)' : 'var(--fg-1)' }}>
              <span style={{ width: 10, color: 'var(--fg-3)' }}>{node.glyph}</span>
              <span className="flex1 truncate">{node.label}</span>
              {node.count != null && <span className="mono-dim" style={{ fontSize: 10 }}>{node.count}</span>}
              {node.key && <Key dim>{node.key}</Key>}
            </div>
          );
        }
        if (node.kind === 'folder') {
          const open = node.open;
          return (
            <React.Fragment key={idx}>
              <div className="row" style={{ height: rowH, padding: '0 12px', gap: 8, color: node.dim ? 'var(--fg-3)' : 'var(--fg-1)' }}>
                <span style={{ width: 10, color: 'var(--fg-3)' }}>{open ? '▾' : '▸'}</span>
                <Dot color={node.dot} size={6} />
                <span className="flex1 truncate">{node.label}</span>
                <span className="mono-dim" style={{ fontSize: 10 }}>{node.count}</span>
                {node.key && <Key dim>{node.key}</Key>}
              </div>
              {open && node.children && node.children.map((c, i) => (
                <div key={i} className="row" style={{
                  height: rowH, padding: '0 12px 0 30px', gap: 8,
                  background: c.active ? 'rgba(242,169,59,0.10)' : 'transparent',
                  color: c.active ? 'var(--amber)' : 'var(--fg-1)',
                  borderLeft: c.active ? '2px solid var(--amber)' : '2px solid transparent',
                }}>
                  <Dot color={c.dot} size={5} />
                  <span className="flex1 truncate">{c.label}</span>
                  <span className="mono-dim" style={{ fontSize: 10, color: c.active ? 'var(--amber-mid)' : 'var(--fg-3)' }}>{c.count}</span>
                </div>
              ))}
            </React.Fragment>
          );
        }
        return null;
      })}
    </div>
  );
};

// Paper row — table-density list row
const PaperRow = ({ p, selected, dense }) => {
  const h = dense ? 26 : 30;
  return (
    <div className="row" style={{
      height: h, padding: '0 12px', gap: 12,
      background: selected ? 'rgba(242,169,59,0.10)' : 'transparent',
      color: selected ? 'var(--amber)' : 'var(--fg)',
      borderLeft: selected ? '2px solid var(--amber)' : '2px solid transparent',
      borderBottom: '1px solid var(--border)',
    }}>
      <span style={{ width: 36, fontSize: 10, color: selected ? 'var(--amber-mid)' : 'var(--fg-3)' }}>{p.year}</span>
      <span style={{ width: 56, fontSize: 10, color: selected ? 'var(--amber-mid)' : 'var(--fg-2)' }}>{p.venue}</span>
      <span className="flex1 truncate" style={{ fontWeight: selected ? 500 : 400 }}>{p.title}</span>
      <span style={{ width: 130, fontSize: 10, color: 'var(--fg-3)' }} className="truncate">{p.authors.slice(0, 2).join(', ')}{p.authors.length > 2 ? ' +' + (p.authors.length - 2) : ''}</span>
      <span style={{ width: 56, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.citations >= 1000 ? (p.citations / 1000).toFixed(1) + 'k' : p.citations}</span>
      <span style={{ width: 28, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.note ? '◆' + p.note : '·'}</span>
      <span style={{ width: 28, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.annotations || '·'}</span>
      <span style={{ width: 54, fontSize: 9, color: p.status === 'UNREAD' ? 'var(--amber)' : p.status === 'READING' ? 'var(--amber-bright)' : 'var(--fg-3)', textAlign: 'right', letterSpacing: '0.1em' }}>{p.status || '·'}</span>
    </div>
  );
};

const PaperListHeader = () => (
  <div className="row hair-b" style={{ height: 22, padding: '0 12px', gap: 12, background: 'var(--bg-1)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--fg-3)', textTransform: 'uppercase' }}>
    <span style={{ width: 36 }}>YEAR</span>
    <span style={{ width: 56 }}>VENUE</span>
    <span className="flex1">TITLE</span>
    <span style={{ width: 130 }}>AUTHORS</span>
    <span style={{ width: 56, textAlign: 'right' }}>CITES</span>
    <span style={{ width: 28, textAlign: 'right' }}>NOTE</span>
    <span style={{ width: 28, textAlign: 'right' }}>ANN</span>
    <span style={{ width: 54, textAlign: 'right' }}>STATUS</span>
  </div>
);

// CollapsiblePane — a vertical pane (left or right side of the editor) with
// a closable header. When collapsed it shrinks to a thin strip with a
// rotated label and a chevron to re-open.
const CollapsiblePane = ({ width = 320, side = 'right', label, vertical, defaultOpen = true, headerExtra, children, hairSide }) => {
  const [open, setOpen] = React.useState(defaultOpen);
  const v = vertical ?? label;
  const hairClass = hairSide || (side === 'right' ? 'hair-l' : 'hair-r');
  if (!open) {
    return (
      <div className={hairClass} style={{
        width: 14, flexShrink: 0, background: 'var(--panel)',
        display: 'flex', flexDirection: 'column', alignItems: 'center',
        justifyContent: 'center', cursor: 'pointer',
      }} onClick={() => setOpen(true)} title={"Open " + v}>
        <span style={{ color: 'var(--amber-mid)', fontSize: 11, lineHeight: 1 }}>{side === 'right' ? '◂' : '▸'}</span>
      </div>
    );
  }
  return (
    <div className={hairClass} style={{ width, flexShrink: 0, background: 'var(--panel)', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div className="row hair-b" style={{ height: 26, padding: '0 10px', gap: 6, background: 'var(--bg-1)' }}>
        <Label hot>{label}</Label>
        <div className="flex1" />
        {headerExtra}
        <span onClick={() => setOpen(false)} style={{ cursor: 'pointer', color: 'var(--fg-2)', fontSize: 11, padding: '0 4px' }} title="Collapse">{side === 'right' ? '▸' : '◂'}</span>
        <span style={{ color: 'var(--fg-3)', fontSize: 11 }}>×</span>
      </div>
      <div className="flex1" style={{ overflow: 'auto' }}>{children}</div>
    </div>
  );
};

// Inline chip-pill input with autocomplete dropdown. Used in Discover.
const PillInput = ({ pills, prompt, dropdown }) => (
  <div style={{ position: 'relative' }}>
    <div className="row" style={{
      border: '1px solid var(--amber-dim)', background: 'var(--bg-1)',
      padding: '6px 8px', gap: 6, minHeight: 32, flexWrap: 'wrap', alignItems: 'center',
    }}>
      <span className="mono-dim" style={{ fontSize: 10, color: 'var(--amber-mid)' }}>:seed</span>
      {pills.map((p, i) => (
        <span key={i} className="row" style={{
          height: 20, padding: '0 6px', gap: 6,
          background: p.solid ? 'var(--amber)' : 'transparent',
          color: p.solid ? 'var(--bg)' : 'var(--fg-1)',
          border: '1px solid ' + (p.solid ? 'var(--amber)' : 'var(--border-2)'),
          fontSize: 11,
        }}>
          <span style={{ color: p.solid ? 'var(--bg)' : 'var(--amber)' }}>{p.prefix || ''}</span>
          <span>{p.label}</span>
          {p.meta && <span className="mono-dim" style={{ fontSize: 9, color: p.solid ? 'rgba(0,0,0,0.5)' : 'var(--fg-3)' }}>{p.meta}</span>}
          <span style={{ color: p.solid ? 'rgba(0,0,0,0.55)' : 'var(--fg-3)', fontSize: 10, cursor: 'pointer' }}>×</span>
        </span>
      ))}
      <span className="row" style={{ gap: 4 }}>
        <span style={{ fontSize: 11, color: 'var(--fg)' }}>{prompt}</span>
        <span style={{ width: 1, height: 14, background: 'var(--amber)' }} />
      </span>
      <div className="flex1" />
      <span className="mono-dim nowrap" style={{ fontSize: 10 }}>↑↓ pick · ⏎ add · ⌫ remove</span>
    </div>
    {dropdown}
  </div>
);

// Autocomplete dropdown content for PillInput.
const PillDropdown = ({ groups }) => (
  <div style={{
    position: 'absolute', top: 'calc(100% + 4px)', left: 0, right: 0,
    background: 'var(--bg-1)', border: '1px solid var(--amber-dim)',
    boxShadow: '0 8px 32px rgba(0,0,0,0.5)', zIndex: 20,
    maxHeight: 320, overflow: 'auto',
  }}>
    {groups.map((g, gi) => (
      <div key={gi} style={{ borderTop: gi > 0 ? '1px solid var(--border)' : 'none' }}>
        <div className="row" style={{ padding: '4px 10px', background: 'var(--bg-2)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--amber-mid)' }}>
          <span>{g.label}</span>
          <div className="flex1" />
          <span style={{ color: 'var(--fg-3)' }}>{g.count} match</span>
        </div>
        {g.items.map((it, i) => (
          <div key={i} className="row" style={{
            padding: '5px 10px', gap: 8, fontSize: 11,
            background: (gi === 0 && i === 0) ? 'rgba(242,169,59,0.10)' : 'transparent',
            color: (gi === 0 && i === 0) ? 'var(--amber)' : 'var(--fg-1)',
            borderLeft: (gi === 0 && i === 0) ? '2px solid var(--amber)' : '2px solid transparent',
          }}>
            <span style={{ width: 14, color: 'var(--amber)' }}>{it.glyph}</span>
            <span style={{ fontWeight: 500 }}>{it.title}</span>
            <span className="mono-dim" style={{ fontSize: 10, color: 'var(--fg-3)' }}>{it.sub}</span>
            <div className="flex1" />
            {it.count && <span className="mono-dim" style={{ fontSize: 10 }}>{it.count}</span>}
          </div>
        ))}
      </div>
    ))}
    <div className="row hair-t" style={{ padding: '4px 10px', background: 'var(--bg-1)', fontSize: 9, color: 'var(--fg-3)', letterSpacing: '0.1em' }}>
      <span>SCOPE: @paper · /folder · #tag · author:name · venue:NeurIPS · year:&gt;2022</span>
      <div className="flex1" />
      <Key dim>esc</Key>
    </div>
  </div>
);

Object.assign(window, { Key, Label, Btn, Chip, HR, Dotted, Dot, Gauge, TitleBar, StatusBar, VaultTree, PaperRow, PaperListHeader, CollapsiblePane, PillInput, PillDropdown });
