// Layout A v2 — Tri-pane IDE
// Activity rail (mode switch only) · Explorer (collapsible) · Editor · Inspector (collapsible)

const ActivityRail = ({ active = 'V' }) => {
  const items = [
    { id: 'V', letters: 'VAULT' },
    { id: 'F', letters: 'FIND'  },
    { id: 'R', letters: 'READ'  },
    { id: 'G', letters: 'GRAPH' },
    { id: 'A', letters: 'ASK'   },
    { id: 'S', letters: 'STUDY' },
  ];
  return (
    <div className="hair-r" style={{ width: 44, flexShrink: 0, background: 'var(--panel)', display: 'flex', flexDirection: 'column' }}>
      <div style={{ height: 28, display: 'flex', alignItems: 'center', justifyContent: 'center', borderBottom: '1px solid var(--border)', color: 'var(--amber)' }}>
        <span style={{ fontWeight: 700, fontSize: 11, letterSpacing: '0.05em' }}>1O1</span>
      </div>
      {items.map(it => {
        const on = it.id === active;
        return (
          <div key={it.id} style={{
            padding: '14px 0', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6,
            color: on ? 'var(--amber)' : 'var(--fg-3)',
            background: on ? 'rgba(242,169,59,0.06)' : 'transparent',
            borderLeft: on ? '2px solid var(--amber)' : '2px solid transparent',
            cursor: 'pointer',
          }}>
            <div style={{ writingMode: 'vertical-rl', transform: 'rotate(180deg)', fontSize: 9, letterSpacing: '0.25em', fontWeight: on ? 600 : 400 }}>
              {it.letters}
            </div>
            <Key dim={!on}>{it.id}</Key>
          </div>
        );
      })}
      <div className="flex1" />
      <div style={{ padding: '12px 0', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8, color: 'var(--fg-3)' }}>
        <div style={{ fontSize: 11 }}>⚙</div>
        <Key dim>?</Key>
      </div>
    </div>
  );
};

const ExplorerPane = () => (
  <CollapsiblePane width={240} side="left" label="EXPLORER" vertical="EXPLORER · VAULT"
    headerExtra={<>
      <span style={{ color: 'var(--fg-3)', fontSize: 11 }}>+</span>
      <span style={{ color: 'var(--fg-3)', fontSize: 11 }}>⤓</span>
    </>}>
    <div style={{ padding: '6px 8px' }}>
      <div className="row" style={{ border: '1px solid var(--border-2)', height: 22, padding: '0 6px', gap: 6 }}>
        <span style={{ color: 'var(--fg-3)', fontSize: 11 }}>/</span>
        <span className="flex1 truncate" style={{ fontSize: 11, color: 'var(--fg-2)' }}>filter vault…</span>
        <Key dim>⌘P</Key>
      </div>
    </div>
    <VaultTree />
  </CollapsiblePane>
);

const TabStrip = ({ tabs, active }) => (
  <div className="row hair-b" style={{ height: 26, background: 'var(--bg-1)', flexShrink: 0 }}>
    {tabs.map((t, i) => {
      const on = i === active;
      return (
        <div key={i} className="row" style={{
          height: '100%', padding: '0 12px', gap: 8,
          background: on ? 'var(--bg)' : 'transparent',
          color: on ? 'var(--amber)' : 'var(--fg-2)',
          borderRight: '1px solid var(--border)',
          borderTop: on ? '1px solid var(--amber)' : '1px solid transparent',
          fontSize: 11, flexShrink: 0,
        }}>
          <span style={{ fontSize: 9, color: on ? 'var(--amber-mid)' : 'var(--fg-3)' }}>{t.icon}</span>
          <span className="truncate" style={{ maxWidth: 220 }}>{t.label}</span>
          {t.dirty && <span style={{ color: 'var(--amber)' }}>●</span>}
          <span style={{ color: 'var(--fg-3)', marginLeft: 4 }}>×</span>
        </div>
      );
    })}
    <div className="flex1" />
    <div className="row gap-8" style={{ padding: '0 10px', fontSize: 10, color: 'var(--fg-3)' }}>
      <span>⇆ split</span>
      <span>⊟ layout</span>
    </div>
  </div>
);

const ShellA = ({ mode = 'V', tabs, active, inspector, inspectorLabel = 'INSPECTOR', inspectorOpen = true, explorerOpen = true, children, status }) => (
  <div className="crt" style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
    {/* TITLE BAR */}
    <div className="row hair-b" style={{ height: 28, background: 'var(--bg-1)', padding: '0 10px', gap: 12, flexShrink: 0 }}>
      <div className="row gap-6">
        <span style={{ width: 8, height: 8, background: 'var(--red)' }} />
        <span style={{ width: 8, height: 8, background: 'var(--amber-mid)' }} />
        <span style={{ width: 8, height: 8, background: 'var(--green)' }} />
      </div>
      <span style={{ color: 'var(--fg-3)' }}>·</span>
      <div className="row gap-6" style={{ fontSize: 11 }}>
        <span style={{ color: 'var(--amber)' }}>1O1</span>
        <span style={{ color: 'var(--fg-3)' }}>/</span>
        <span style={{ color: 'var(--fg-2)' }}>vault</span>
        <span style={{ color: 'var(--fg-3)' }}>/</span>
        <span style={{ color: 'var(--fg-2)' }}>transformers</span>
        <span style={{ color: 'var(--fg-3)' }}>/</span>
        <span style={{ color: 'var(--fg-1)' }}>attention</span>
      </div>
      <div className="flex1" />
      <div className="row" style={{ border: '1px solid var(--border-2)', height: 20, padding: '0 8px', gap: 8, minWidth: 360, flexShrink: 0 }}>
        <span style={{ color: 'var(--fg-3)', fontSize: 11 }}>⌕</span>
        <span className="flex1 truncate" style={{ fontSize: 11, color: 'var(--fg-2)' }}>search vault, run command, ask…</span>
        <Key dim>⌘K</Key>
      </div>
      <div className="flex1" />
      <div className="row gap-12" style={{ fontSize: 10, color: 'var(--fg-2)' }}>
        <span><span style={{ color: 'var(--green)' }}>●</span> sync</span>
        <span>234 papers</span>
        <span style={{ color: 'var(--fg-3)' }}>@gengeltje</span>
      </div>
    </div>

    {/* BODY */}
    <div className="row flex1" style={{ minHeight: 0, alignItems: 'stretch' }}>
      <ActivityRail active={mode} />
      {explorerOpen ? <ExplorerPane /> : <CollapsedStrip side="left" label="EXPLORER · VAULT" />}
      <div className="flex1 col" style={{ minWidth: 0 }}>
        {tabs && <TabStrip tabs={tabs} active={active} />}
        <div className="flex1" style={{ minHeight: 0, overflow: 'hidden', background: 'var(--bg)' }}>
          {children}
        </div>
      </div>
      {inspector && (
        inspectorOpen
          ? <CollapsiblePane width={320} side="right" label={inspectorLabel} defaultOpen>{inspector}</CollapsiblePane>
          : <CollapsedStrip side="right" label={inspectorLabel} />
      )}
    </div>

    {/* STATUS BAR */}
    <StatusBar
      left={status?.left || [
        <><span style={{ color: 'var(--green)' }}>●</span> connected</>,
        <>vault · 234 papers · 12 unread</>,
        <>cite: BibTeX</>,
      ]}
      right={status?.right || [
        <>[J K] navigate</>,
        <>[O] open</>,
        <>[⌘K] palette</>,
        <>1O1 v0.4.2</>,
      ]}
    />
  </div>
);

// Static collapsed-strip — minimal: a thin rail with just a chevron handle.
// Hover/click reveals the pane (in the live CollapsiblePane component).
const CollapsedStrip = ({ side, label }) => (
  <div className={side === 'right' ? 'hair-l' : 'hair-r'} title={"Open " + label} style={{
    width: 14, flexShrink: 0, background: 'var(--panel)',
    display: 'flex', flexDirection: 'column', alignItems: 'center',
    justifyContent: 'center', cursor: 'pointer',
  }}>
    <span style={{ color: 'var(--amber-mid)', fontSize: 11, lineHeight: 1 }}>{side === 'right' ? '◂' : '▸'}</span>
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN A1 — HOME
// ─────────────────────────────────────────────────────────────────────────
const ScreenA_Home = () => {
  const papers = window.PAPERS.slice(0, 12);
  const tabs = [{ icon: '◆', label: '/transformers/attention', dirty: true }];
  return (
    <ShellA mode="V" tabs={tabs} active={0} inspector={<InspectorAttentionFolder />}>
      <div className="col" style={{ height: '100%' }}>
        <div className="hair-b" style={{ padding: '14px 18px 12px', background: 'var(--bg-1)' }}>
          <div className="row gap-8" style={{ marginBottom: 6 }}>
            <Label>FOLDER</Label>
            <span className="mono-dim" style={{ fontSize: 10 }}>vault / transformers / attention</span>
          </div>
          <div className="row gap-12" style={{ alignItems: 'baseline' }}>
            <h1 style={{ margin: 0, fontSize: 22, fontWeight: 600, color: 'var(--amber)', letterSpacing: '-0.01em' }}>attention</h1>
            <span className="mono-dim nowrap" style={{ fontSize: 11 }}>22 papers · 3 unread · last add 2d</span>
            <div className="flex1" />
            <div className="row gap-6">
              <Btn>＋ ADD</Btn>
              <Btn>↑ IMPORT</Btn>
              <Btn>↓ EXPORT .BIB</Btn>
              <Btn ghost>⋯</Btn>
            </div>
          </div>
          <div className="row gap-6 nowrap" style={{ marginTop: 12, overflow: 'hidden' }}>
            <Chip hot>foundational ×8</Chip>
            <Chip>transformer ×22</Chip>
            <Chip>attention ×22</Chip>
            <Chip>NeurIPS ×6</Chip>
            <Chip>ICLR ×4</Chip>
            <Chip>2017–2024</Chip>
            <div className="flex1" />
            <span className="mono-dim nowrap" style={{ fontSize: 10 }}>filter [/]</span>
            <span className="mono-dim nowrap" style={{ fontSize: 10 }}>sort [S] · recent</span>
            <span className="mono-dim nowrap" style={{ fontSize: 10 }}>view [⌘1] · list</span>
          </div>
        </div>

        <div className="row hair-b nowrap" style={{ height: 28, background: 'var(--bg-1)', padding: '0 18px', gap: 18, fontSize: 11, color: 'var(--fg-2)' }}>
          <span className="nowrap" style={{ color: 'var(--amber)', borderBottom: '1px solid var(--amber)', height: '100%', display: 'inline-flex', alignItems: 'center', paddingTop: 1 }}>● PAPERS <span className="mono-dim" style={{ marginLeft: 4, color: 'var(--amber-mid)' }}>22</span></span>
          <span className="nowrap">○ NOTES <span className="mono-dim" style={{ marginLeft: 4 }}>14</span></span>
          <span className="nowrap">○ ANNOTATIONS <span className="mono-dim" style={{ marginLeft: 4 }}>87</span></span>
          <span className="nowrap">○ GRAPH</span>
          <span className="nowrap">○ Q&amp;A</span>
          <div className="flex1" />
          <span className="mono-dim nowrap" style={{ fontSize: 10 }}>[1] [2] [3] [4] [5]</span>
        </div>

        <PaperListHeader />
        <div className="flex1" style={{ overflow: 'auto' }}>
          {papers.map((p, i) => (
            <PaperRowOpenable key={p.id} p={p} selected={i === 0} hovered={i === 1} />
          ))}
          {[...Array(8)].map((_, i) => (
            <div key={'e' + i} className="row" style={{ height: 22, padding: '0 12px', gap: 12, borderBottom: '1px solid var(--border)', color: 'var(--fg-4)' }}>
              <span style={{ width: 36, fontSize: 10 }}>·</span>
              <span style={{ width: 56, fontSize: 10 }}>·</span>
              <span className="flex1" />
            </div>
          ))}
        </div>
      </div>
    </ShellA>
  );
};

// Paper row that surfaces an "↵ open" hint on hover/selection so the
// path from list → reader is obvious.
const PaperRowOpenable = ({ p, selected, hovered }) => {
  const h = 30;
  const showOpen = selected || hovered;
  return (
    <div className="row" style={{
      height: h, padding: '0 12px', gap: 12,
      background: selected ? 'rgba(242,169,59,0.10)' : hovered ? 'rgba(242,169,59,0.04)' : 'transparent',
      color: selected ? 'var(--amber)' : 'var(--fg)',
      borderLeft: selected ? '2px solid var(--amber)' : '2px solid transparent',
      borderBottom: '1px solid var(--border)',
      position: 'relative',
    }}>
      <span style={{ width: 36, fontSize: 10, color: selected ? 'var(--amber-mid)' : 'var(--fg-3)' }}>{p.year}</span>
      <span style={{ width: 56, fontSize: 10, color: selected ? 'var(--amber-mid)' : 'var(--fg-2)' }}>{p.venue}</span>
      <span className="flex1 truncate" style={{ fontWeight: selected ? 500 : 400 }}>{p.title}</span>
      <span style={{ width: 130, fontSize: 10, color: 'var(--fg-3)' }} className="truncate">{p.authors.slice(0, 2).join(', ')}{p.authors.length > 2 ? ' +' + (p.authors.length - 2) : ''}</span>
      <span style={{ width: 56, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.citations >= 1000 ? (p.citations / 1000).toFixed(1) + 'k' : p.citations}</span>
      <span style={{ width: 28, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.note ? '◆' + p.note : '·'}</span>
      <span style={{ width: 28, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.annotations || '·'}</span>
      <span style={{ width: 54, fontSize: 9, color: p.status === 'UNREAD' ? 'var(--amber)' : p.status === 'READING' ? 'var(--amber-bright)' : 'var(--fg-3)', textAlign: 'right', letterSpacing: '0.1em' }}>{p.status || '·'}</span>
      {showOpen && (
        <span className="row" style={{
          position: 'absolute', right: 8, top: '50%', transform: 'translateY(-50%)',
          gap: 4, background: 'var(--bg)', border: '1px solid var(--amber-dim)', padding: '0 6px', height: 16,
          fontSize: 9, color: 'var(--amber)', letterSpacing: '0.1em',
        }}>
          <Key dim>↵</Key><span>READ</span>
          <span style={{ color: 'var(--fg-3)' }}>·</span>
          <Key dim>O</Key><span>OPEN</span>
        </span>
      )}
    </div>
  );
};

const InspectorAttentionFolder = () => (
  <>
    <div className="hair-b" style={{ padding: '12px 14px', background: 'var(--bg-1)' }}>
      <div style={{ fontSize: 12, color: 'var(--amber)' }}>● attention</div>
      <div className="mono-dim" style={{ fontSize: 10, marginTop: 2 }}>folder · 22 papers</div>
    </div>

    <div style={{ padding: '14px 14px 0' }}>
      <Label>READING STATS</Label>
      <div className="col gap-4" style={{ marginTop: 8, fontSize: 11 }}>
        {[
          ['read',      18, 22],
          ['reading',    2, 22],
          ['unread',     2, 22],
        ].map(([k, v, total]) => (
          <div key={k} className="row gap-8">
            <span style={{ width: 60, color: 'var(--fg-2)' }}>{k}</span>
            <Gauge value={v / total} w={80} />
            <span className="mono-dim" style={{ fontSize: 10 }}>{v} / {total}</span>
          </div>
        ))}
      </div>
    </div>

    <div style={{ padding: '14px 14px 0' }}>
      <Label>AUTHORS · TOP CO-OCCURRING</Label>
      <div className="col gap-4" style={{ marginTop: 8, fontSize: 11 }}>
        {[
          ['A. Vaswani', 6], ['N. Shazeer', 4], ['J. Devlin',  3], ['Y. Bengio',  3], ['K. He', 2], ['M. Caron', 2],
        ].map(([name, n]) => (
          <div key={name} className="row gap-8">
            <span className="flex1">{name}</span>
            <span className="mono-dim" style={{ fontSize: 10 }}>×{n}</span>
            <Key dim>↗</Key>
          </div>
        ))}
      </div>
    </div>

    <div style={{ padding: '14px 14px 0' }}>
      <Label hot>ASK THIS FOLDER</Label>
      <div style={{ marginTop: 8, border: '1px solid var(--border-2)', padding: 8, background: 'var(--bg-1)' }}>
        <div style={{ fontSize: 11, color: 'var(--fg-2)', lineHeight: 1.5 }}>
          What are the main arguments against<br/>scaled dot-product attention's<br/>O(n²) complexity?
        </div>
        <div className="row" style={{ marginTop: 8, gap: 6 }}>
          <Btn primary>RUN ↵</Btn>
          <Btn ghost>cite 22</Btn>
        </div>
      </div>
    </div>

    <div className="flex1" />

    <div className="hair-t" style={{ padding: '10px 14px', background: 'var(--bg-1)' }}>
      <Label>RECENTLY OPENED</Label>
      <div className="col" style={{ marginTop: 6, fontSize: 10, color: 'var(--fg-2)', gap: 2 }}>
        <div>▸ Vaswani 2017 — 2h</div>
        <div>▸ Devlin 2019 — yesterday</div>
        <div>▸ Tay 2022 — 3d</div>
      </div>
    </div>
  </>
);

// ─────────────────────────────────────────────────────────────────────────
// READER — shared body with TEXT/PDF toggle
// ─────────────────────────────────────────────────────────────────────────
const ReaderHeader = ({ mode = 'TEXT' }) => (
  <div className="hair-b" style={{ padding: '12px 18px', background: 'var(--bg-1)' }}>
    <div className="row gap-8" style={{ marginBottom: 4 }}>
      <Label>PAPER</Label>
      <span className="mono-dim" style={{ fontSize: 10 }}>arXiv:1706.03762 · NeurIPS 2017</span>
      <Chip hot>foundational</Chip>
      <Chip>transformer</Chip>
      <div className="flex1" />
      <span className="mono-dim" style={{ fontSize: 10 }}>cited 134.8k · in vault since 2024-04-12</span>
    </div>
    <div className="row gap-12" style={{ alignItems: 'baseline' }}>
      <h1 style={{ margin: 0, fontSize: 18, fontWeight: 600, color: 'var(--amber)' }}>Attention Is All You Need</h1>
      <span className="mono-mid nowrap truncate" style={{ fontSize: 11 }}>Vaswani · Shazeer · Parmar · Uszkoreit · Jones · Gomez · Kaiser · Polosukhin</span>
    </div>
    {/* View toggle + actions */}
    <div className="row gap-10" style={{ marginTop: 10, alignItems: 'center' }}>
      <div className="row" style={{ border: '1px solid var(--border-2)' }}>
        <ViewTab on={mode === 'TEXT'} k="T" label="TEXT" />
        <ViewTab on={mode === 'PDF'}  k="P" label="PDF"  />
        <ViewTab k="S" label="SPLIT"  />
      </div>
      <Btn>✎ NOTES</Btn>
      <Btn>⌥ ANNOTATE</Btn>
      <Btn>◇ GRAPH</Btn>
      <Btn>＋ ASK</Btn>
      <div className="flex1" />
      <span className="mono-dim nowrap" style={{ fontSize: 10 }}>[H] highlight · [N] note · [Q] question · [F] flag</span>
    </div>
  </div>
);

const ViewTab = ({ on, k, label }) => (
  <div className="row" style={{
    padding: '0 10px', height: 22, gap: 6, fontSize: 10, letterSpacing: '0.1em',
    background: on ? 'var(--amber)' : 'transparent',
    color: on ? 'var(--bg)' : 'var(--fg-2)',
    borderRight: '1px solid var(--border-2)',
    cursor: 'pointer',
  }}>
    <span style={{ fontWeight: 600 }}>{label}</span>
    <span style={{
      border: '1px solid ' + (on ? 'var(--bg)' : 'var(--amber-dim)'),
      padding: '0 4px', fontSize: 8, fontWeight: 600,
      color: on ? 'var(--bg)' : 'var(--amber)',
    }}>{k}</span>
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN A2 — READER · TEXT mode (margin + inspector both open)
// ─────────────────────────────────────────────────────────────────────────
const ScreenA_Reader = () => {
  const tabs = [
    { icon: '◆', label: '/transformers/attention' },
    { icon: '▤', label: 'Vaswani 2017 · Attention…', dirty: true },
  ];
  return (
    <ShellA
      mode="R" tabs={tabs} active={1}
      inspector={<InspectorReader />}
    >
      <div className="col" style={{ height: '100%' }}>
        <ReaderHeader mode="TEXT" />

        <div className="row flex1" style={{ minHeight: 0, alignItems: 'stretch' }}>
          {/* Text page — paper-styled, matches PDF aesthetic */}
          <div className="flex1" style={{ background: 'var(--bg-2)', overflow: 'auto', padding: '20px 24px', display: 'flex', justifyContent: 'center', borderRight: '1px solid var(--border)' }}>
            <TextPage />
          </div>

          {/* Margin column — collapsible */}
          <CollapsiblePane width={260} side="right" label="MARGIN · §1" vertical="MARGIN · 27 marks">
            <MarginContent />
          </CollapsiblePane>
        </div>

        <div className="row hair-t" style={{ height: 26, background: 'var(--bg-1)', padding: '0 18px', gap: 14, fontSize: 10, color: 'var(--fg-2)' }}>
          <span><span className="k dim">◄</span> p.2 / 15 <span className="k dim">►</span></span>
          <span className="mono-dim">scroll · ↑↓ · or [j k]</span>
          <div className="flex1" />
          <span className="mono-dim">selection · 0 chars</span>
          <span><Key dim>H</Key> highlight</span>
          <span><Key dim>N</Key> note</span>
          <span><Key dim>Q</Key> question</span>
          <span><Key dim>⌘⏎</Key> ask</span>
        </div>
      </div>
    </ShellA>
  );
};

const MarginContent = () => (
  <div style={{ padding: 12, fontSize: 10.5, lineHeight: 1.5, color: 'var(--fg-1)' }}>
    <div style={{ marginTop: 0 }}>
      <div className="row gap-4" style={{ color: 'var(--amber-mid)', fontSize: 9, letterSpacing: '0.1em', marginBottom: 4 }}>
        <span>◆ NOTE · ¶3</span>
        <span style={{ color: 'var(--fg-3)' }}>· me · 2d</span>
      </div>
      <div style={{ border: '1px solid var(--border-2)', borderLeft: '2px solid var(--amber)', padding: '6px 8px' }}>
        Key motivation — RNN parallelism wall.<br/>See Chen 2018 §3 for follow-up.
      </div>
    </div>
    <div style={{ marginTop: 16 }}>
      <div className="row gap-4" style={{ color: 'var(--amber-mid)', fontSize: 9, letterSpacing: '0.1em', marginBottom: 4 }}>
        <span>? QUESTION · ¶5</span>
        <span style={{ color: 'var(--fg-3)' }}>· me · 2d</span>
      </div>
      <div style={{ border: '1px solid var(--border-2)', borderLeft: '2px solid var(--amber-bright)', padding: '6px 8px', color: 'var(--amber-bright)' }}>
        Q: how does this compare to ByteNet / ConvS2S in §2?
      </div>
      <div className="row gap-6" style={{ marginTop: 6, fontSize: 9 }}>
        <span className="mono-dim">[A] ask vault</span>
        <span className="mono-dim">[L] link paper</span>
      </div>
    </div>
    <div style={{ marginTop: 16 }}>
      <div className="row gap-4" style={{ color: 'var(--amber-mid)', fontSize: 9, letterSpacing: '0.1em', marginBottom: 4 }}>
        <span>◆ HIGHLIGHT · ¶8</span>
        <span style={{ color: 'var(--fg-3)' }}>· me · today</span>
      </div>
      <div style={{ border: '1px solid var(--border-2)', borderLeft: '2px solid var(--amber-mid)', padding: '6px 8px' }}>
        "Self-attention … relating different positions of a single sequence" → tie to DINO's CLS-token attention maps.
      </div>
    </div>
  </div>
);

const InspectorReader = () => (
  <>
    <div className="hair-b" style={{ padding: '10px 14px', background: 'var(--bg-1)' }}>
      <div style={{ fontSize: 11, color: 'var(--amber)' }}>▤ Vaswani 2017</div>
      <div className="mono-dim" style={{ fontSize: 10, marginTop: 2 }}>paper · ¶ 3 · selected</div>
    </div>

    <div style={{ padding: '12px 14px 0' }}>
      <div className="row" style={{ gap: 6, marginBottom: 8 }}>
        <Label hot>LINEAGE</Label>
        <div className="flex1" />
        <span className="mono-dim" style={{ fontSize: 9 }}>3 ← · → 5</span>
      </div>
      <div style={{ fontSize: 10, color: 'var(--fg-2)', lineHeight: 1.6 }}>
        <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em' }}>CITES ←</div>
        {[
          ['Bahdanau 2014','soft alignment seed', true],
          ['Luong 2015','dot-prod variant'],
          ['Cheng 2016','intra-attention'],
        ].map(([t, why, owned]) => (
          <div key={t} className="row gap-6" style={{ padding: '3px 0' }}>
            <span style={{ width: 8, color: owned ? 'var(--amber)' : 'var(--fg-3)' }}>{owned ? '●' : '○'}</span>
            <span style={{ color: 'var(--fg-1)' }}>{t}</span>
            <span className="mono-dim" style={{ fontSize: 9 }}>{why}</span>
          </div>
        ))}
        <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em', marginTop: 8 }}>CITED BY →</div>
        {[
          ['Devlin 2019', 'BERT', true],
          ['Radford 2019', 'GPT-2'],
          ['Dosovitskiy 2021','ViT', true],
          ['Brown 2020', 'GPT-3'],
          ['Touvron 2023','LLaMA'],
        ].map(([t, why, owned]) => (
          <div key={t} className="row gap-6" style={{ padding: '3px 0' }}>
            <span style={{ width: 8, color: owned ? 'var(--amber)' : 'var(--fg-3)' }}>{owned ? '●' : '○'}</span>
            <span style={{ color: 'var(--fg-1)' }}>{t}</span>
            <span className="mono-dim" style={{ fontSize: 9 }}>{why}</span>
          </div>
        ))}
        <div style={{ marginTop: 6 }}>
          <Btn ghost style={{ width: '100%', justifyContent: 'center' }}>OPEN GRAPH ◇</Btn>
        </div>
      </div>
    </div>

    <div style={{ padding: '14px 14px 0' }}>
      <Label hot>ASK THIS PAPER</Label>
      <div style={{ marginTop: 8, border: '1px solid var(--amber-dim)', padding: 8, background: 'rgba(242,169,59,0.04)' }}>
        <div style={{ fontSize: 10, color: 'var(--fg-3)', marginBottom: 4, letterSpacing: '0.1em' }}>YOU</div>
        <div style={{ fontSize: 11, color: 'var(--fg-1)', lineHeight: 1.5 }}>
          Explain scaled dot-product attention vs additive attention.
        </div>
        <div className="mono-dim" style={{ fontSize: 9, marginTop: 6, color: 'var(--amber)' }}>1O1 · cites ¶8, eq.(1), §3.2.1</div>
        <div style={{ fontSize: 10.5, color: 'var(--fg-2)', lineHeight: 1.55, marginTop: 4 }}>
          Scaled dot-product computes Q·Kᵀ / √dₖ and softmaxes [¶8]. Additive uses a feed-forward net with a single hidden layer [§3.2.1]. The authors find dot-product faster in practice…
        </div>
        <div className="row gap-6" style={{ marginTop: 8 }}>
          <Btn primary>↵</Btn>
          <Btn ghost>＋ follow-up</Btn>
          <div className="flex1" />
          <span className="mono-dim" style={{ fontSize: 9 }}>3 / 8 left</span>
        </div>
      </div>
    </div>

    <div className="flex1" />

    <div className="hair-t" style={{ padding: '10px 14px', background: 'var(--bg-1)' }}>
      <Label>METADATA</Label>
      <div className="col" style={{ marginTop: 6, fontSize: 10, color: 'var(--fg-2)', gap: 2 }}>
        <div><span style={{ color: 'var(--fg-3)' }}>doi   </span> 10.48550/arXiv.1706.03762</div>
        <div><span style={{ color: 'var(--fg-3)' }}>venue </span> NeurIPS 2017 · Long Beach</div>
        <div><span style={{ color: 'var(--fg-3)' }}>pages </span> 15 · 5,084 words</div>
        <div><span style={{ color: 'var(--fg-3)' }}>cite  </span> vaswani2017attention</div>
      </div>
    </div>
  </>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN A2-alt — READER · PDF mode (margin collapsed, inspector open)
// ─────────────────────────────────────────────────────────────────────────
const ScreenA_Reader_PDF = () => {
  const tabs = [
    { icon: '◆', label: '/transformers/attention' },
    { icon: '▤', label: 'Vaswani 2017 · Attention…', dirty: true },
  ];
  return (
    <ShellA mode="R" tabs={tabs} active={1} inspector={<InspectorReader />}>
      <div className="col" style={{ height: '100%' }}>
        <ReaderHeader mode="PDF" />

        <div className="row flex1" style={{ minHeight: 0, alignItems: 'stretch' }}>
          {/* PDF column */}
          <div className="flex1" style={{ background: 'var(--bg-2)', overflow: 'auto', padding: '20px 24px', display: 'flex', justifyContent: 'center' }}>
            <PDFPage />
          </div>

          {/* Margin collapsed strip — demonstrate that panes close */}
          <CollapsedStrip side="right" label="MARGIN · 27 marks" />
        </div>

        <div className="row hair-t" style={{ height: 26, background: 'var(--bg-1)', padding: '0 18px', gap: 14, fontSize: 10, color: 'var(--fg-2)' }}>
          <span><span className="k dim">◄</span> p.2 / 15 <span className="k dim">►</span></span>
          <span className="mono-dim">zoom 100% · ⌘0 fit</span>
          <div className="flex1" />
          <span className="mono-dim">PDF · text layer ON · render via pdf.js</span>
          <span><Key dim>T</Key> text view</span>
        </div>
      </div>
    </ShellA>
  );
};

// Faux PDF page — clean, paper-like rectangle with two columns of dim text.
const PDFPage = () => (
  <div style={paperPageStyle(560)}>
    <div style={{ textAlign: 'center', marginBottom: 24 }}>
      <div style={{ fontSize: 14, fontWeight: 700, letterSpacing: '0.02em', marginBottom: 4 }}>Attention Is All You Need</div>
      <div style={{ fontSize: 9, marginBottom: 4 }}>
        Ashish Vaswani · Noam Shazeer · Niki Parmar · Jakob Uszkoreit<br/>
        Llion Jones · Aidan N. Gomez · Łukasz Kaiser · Illia Polosukhin
      </div>
      <div style={{ fontSize: 8, color: '#5a4630' }}>Google Brain · Google Research · University of Toronto</div>
    </div>
    <div style={{ fontWeight: 700, fontSize: 9, marginBottom: 4 }}>Abstract</div>
    <p style={{ margin: '0 0 12px' }}>
      The dominant sequence transduction models are based on complex recurrent or convolutional neural networks
      that include an encoder and a decoder. The best performing models also connect the encoder and decoder
      through an attention mechanism. We propose a new simple network architecture, the Transformer, based solely
      on attention mechanisms, dispensing with recurrence and convolutions entirely.
    </p>
    <div style={{ fontWeight: 700, fontSize: 10, margin: '14px 0 6px' }}>1   Introduction</div>
    {/* Two columns */}
    <div style={{ columnCount: 2, columnGap: 18 }}>
      <p style={{ margin: '0 0 8px' }}>
        Recurrent neural networks, long short-term memory and gated recurrent neural networks in
        particular, have been firmly established as state of the art approaches in sequence
        modeling and transduction problems such as language modeling and machine translation.
      </p>
      <p style={{ margin: '0 0 8px' }}>
        Numerous efforts have since continued to push the boundaries of recurrent language models
        and encoder-decoder architectures.
      </p>
      <p style={{ margin: '0 0 8px', background: 'rgba(220, 160, 60, 0.45)', padding: '2px 4px' }}>
        Recurrent models typically factor computation along the symbol positions of the input and
        output sequences. Aligning the positions to steps in computation time, they generate a
        sequence of hidden states h_t, as a function of the previous hidden state h_{'{t-1}'}.
      </p>
      <p style={{ margin: '0 0 8px' }}>
        This inherently sequential nature precludes parallelization within training examples,
        which becomes critical at longer sequence lengths, as memory constraints limit batching
        across examples.
      </p>
      <p style={{ margin: '0 0 8px' }}>
        Attention mechanisms have become an integral part of compelling sequence modeling and
        transduction models in various tasks, allowing modeling of dependencies without regard
        to their distance in the input or output sequences.
      </p>
      <p style={{ margin: '0 0 8px' }}>
        In this work we propose the Transformer, a model architecture eschewing recurrence and
        instead relying entirely on an attention mechanism to draw global dependencies between
        input and output.
      </p>
    </div>
    <div style={{ marginTop: 16, fontSize: 8, color: '#5a4630', textAlign: 'center' }}>2</div>
  </div>
);

// Paper-styled page used by both PDF mode (2 columns, smaller text) and
// TEXT mode (single wider column, larger text, annotation highlights inline).
function paperPageStyle(width) {
  return {
    width, minHeight: 740, maxWidth: '100%',
    background: '#f0e6c8', color: '#1a1208',
    border: '1px solid rgba(0,0,0,0.2)',
    boxShadow: '0 8px 32px rgba(0,0,0,0.6)',
    padding: '36px 44px',
    fontFamily: '"IBM Plex Mono", monospace',
    fontSize: 11, lineHeight: 1.65,
    flexShrink: 0,
  };
}

// TextPage — extracted-text reading view, same paper aesthetic as PDFPage
// but single column, slightly larger type, and annotation highlights inline.
// Matches PDFPage's width so the two modes stay visually aligned.
const TextPage = () => (
  <div style={{ ...paperPageStyle(560), fontSize: 11 }}>
    <div style={{ fontSize: 9, color: '#7a5a30', letterSpacing: '0.15em', marginBottom: 14 }}>EXTRACTED TEXT · arXiv:1706.03762 · p.1–2 / 15</div>

    {/* Title block — matches PDF layout */}
    <div style={{ textAlign: 'center', marginBottom: 22 }}>
      <div style={{ fontSize: 18, fontWeight: 700, letterSpacing: '-0.01em', marginBottom: 8 }}>Attention Is All You Need</div>
      <div style={{ fontSize: 10.5, marginBottom: 4, color: '#1a1208' }}>
        Ashish Vaswani · Noam Shazeer · Niki Parmar · Jakob Uszkoreit<br/>
        Llion Jones · Aidan N. Gomez · Łukasz Kaiser · Illia Polosukhin
      </div>
      <div style={{ fontSize: 9, color: '#5a4630' }}>Google Brain · Google Research · University of Toronto</div>
    </div>

    {/* Abstract */}
    <div style={{ fontWeight: 700, fontSize: 10.5, marginBottom: 5, letterSpacing: '0.02em' }}>Abstract</div>
    <p style={{ margin: '0 0 16px 0', fontSize: 10.5 }}>
      The dominant sequence transduction models are based on complex recurrent or convolutional neural networks
      that include an encoder and a decoder. The best performing models also connect the encoder and decoder
      through an attention mechanism. We propose a new simple network architecture, the Transformer, based solely
      on attention mechanisms, dispensing with recurrence and convolutions entirely. Experiments on two machine
      translation tasks show these models to be superior in quality while being more parallelizable and requiring
      significantly less time to train.
    </p>

    {window.READER_BODY.map((b, idx) => {
      if (b.heading) {
        return (
          <div key={b.id} style={{ fontWeight: 700, fontSize: 11, margin: idx === 0 ? '0 0 8px' : '18px 0 8px', letterSpacing: '0.02em' }}>
            {b.heading}
          </div>
        );
      }
      const num = b.id.replace('p', '');
      const baseStyle = { margin: '0 0 10px 0', position: 'relative', paddingLeft: 24, fontSize: 10.5 };
      const numStyle = { position: 'absolute', left: 0, top: 2, fontSize: 8.5, color: '#7a5a30', fontVariantNumeric: 'tabular-nums', letterSpacing: '0.05em' };
      if (b.highlight === 'amber') {
        return (
          <p key={b.id} style={{ ...baseStyle, background: 'rgba(220, 160, 60, 0.40)', padding: '4px 10px 4px 28px' }}>
            <span style={numStyle}>¶{num}</span>{b.text}
            <span style={{ display: 'block', fontSize: 8.5, color: '#5a3a18', marginTop: 4, fontStyle: 'italic' }}>◆ note · me · 2d</span>
          </p>
        );
      }
      if (b.highlight === 'strong') {
        return (
          <p key={b.id} style={{ ...baseStyle, background: 'rgba(220, 160, 60, 0.65)', padding: '4px 10px 4px 28px', fontWeight: 500 }}>
            <span style={numStyle}>¶{num}</span>{b.text}
            <span style={{ display: 'block', fontSize: 8.5, color: '#5a3a18', marginTop: 4, fontStyle: 'italic' }}>? question · me · 2d</span>
          </p>
        );
      }
      return (
        <p key={b.id} style={baseStyle}>
          <span style={numStyle}>¶{num}</span>{b.text}
        </p>
      );
    })}

    <div style={{ marginTop: 22, fontSize: 8, color: '#7a5a30', textAlign: 'center', letterSpacing: '0.2em' }}>— PAGE 2 OF 15 —</div>
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN A3 — DISCOVER (chip-pill seed + autocomplete + Search Console)
// ─────────────────────────────────────────────────────────────────────────
const ScreenA_Discover = () => {
  const tabs = [
    { icon: '◆', label: '/transformers/attention' },
    { icon: '▤', label: 'Vaswani 2017' },
    { icon: '✦', label: 'Discover · ssl + dino', dirty: true },
  ];
  const pills = [
    { prefix: '@', label: 'caron2021', meta: 'DINO', solid: true },
    { prefix: '/', label: 'self-supervised', meta: '41p' },
    { prefix: '#', label: 'frontier' },
    { prefix: '', label: 'year:>2022' },
  ];
  const dropdown = (
    <PillDropdown groups={[
      { label: 'PAPERS', count: 12, items: [
        { glyph: '@', title: 'Vision Transformers Need Registers', sub: 'Darcet 2024 · ICLR', count: 'cit 412' },
        { glyph: '@', title: 'I-JEPA',  sub: 'Assran 2023 · CVPR',          count: 'cit 612' },
        { glyph: '@', title: 'DINOv2',  sub: 'Oquab 2024 · TMLR · ★ vault', count: 'cit 1.8k' },
      ]},
      { label: 'FOLDERS', count: 4, items: [
        { glyph: '/', title: 'interpretability',       sub: 'vault folder', count: '28 papers' },
        { glyph: '/', title: 'vision-transformers',    sub: 'vault folder', count: '33 papers' },
      ]},
      { label: 'TAGS', count: 3, items: [
        { glyph: '#', title: 'jepa',         sub: 'tag', count: '6 papers' },
        { glyph: '#', title: 'masked',       sub: 'tag', count: '11 papers' },
      ]},
      { label: 'AUTHORS', count: 2, items: [
        { glyph: '☻', title: 'Mathilde Caron',  sub: 'Meta AI · 7 in vault' },
        { glyph: '☻', title: 'Yann LeCun',      sub: 'Meta AI · 3 in vault' },
      ]},
    ]} />
  );
  return (
    <ShellA mode="F" tabs={tabs} active={2} inspectorLabel="SEARCH CONSOLE" inspector={<InspectorSearchConsole />}>
      <div className="col" style={{ height: '100%' }}>
        <div className="hair-b" style={{ padding: '14px 18px 10px', background: 'var(--bg-1)', position: 'relative', zIndex: 10 }}>
          <div className="row gap-8" style={{ marginBottom: 6 }}>
            <Label hot>DISCOVER · seed-based</Label>
            <span className="mono-dim nowrap" style={{ fontSize: 10 }}>type to refine seed · chips combine with AND</span>
            <div className="flex1" />
            <span className="row gap-6 nowrap" style={{ fontSize: 10, color: 'var(--amber-mid)', border: '1px solid var(--amber-dim)', padding: '0 6px', height: 20, alignItems: 'center' }}>
              <span style={{ color: 'var(--amber)' }}>★</span>
              <span>Saved as</span>
              <span style={{ color: 'var(--amber)' }}>ssl + dino + frontier</span>
              <span className="mono-dim" style={{ fontSize: 9 }}>· in /Pins/Saved searches</span>
              <span style={{ color: 'var(--fg-3)', cursor: 'pointer' }}>✎</span>
            </span>
            <Btn ghost>↺ rerank</Btn>
            <Btn>＋ AGENT</Btn>
          </div>
          <PillInput pills={pills} prompt="caron" dropdown={dropdown} />
          {/* Below pill input — actions including SAVE */}
          <div className="row gap-6" style={{ marginTop: 8, fontSize: 10, color: 'var(--fg-3)' }}>
            <span className="mono-dim">candidates · 8 ranked · last run 02:48 · 3 agents working</span>
            <div className="flex1" />
            <Btn ghost>↻ AUTO-RUN</Btn>
            <Btn ghost>★ SAVED</Btn>
            <Btn ghost>↓ EXPORT .BIB</Btn>
            <Btn primary>＋ ADD STARRED 5</Btn>
          </div>
        </div>

        {/* Feed */}
        <div className="flex1" style={{ overflow: 'auto', padding: '12px 18px' }}>
          {window.DISCOVER_FEED.map(p => (
            <div key={p.id} style={{
              border: '1px solid var(--border)',
              borderLeft: p.owned ? '2px solid var(--green)' : '2px solid var(--amber-dim)',
              padding: '12px 14px', marginBottom: 8,
              background: p.owned ? 'rgba(138,168,74,0.05)' : 'var(--bg-1)',
            }}>
              <div className="row gap-12" style={{ alignItems: 'baseline' }}>
                <span style={{ fontSize: 14, color: 'var(--amber)', fontWeight: 600, minWidth: 36 }}>{p.score.toFixed(2)}</span>
                <div className="flex1" style={{ minWidth: 0 }}>
                  <div className="row gap-8" style={{ alignItems: 'baseline' }}>
                    <span style={{ fontSize: 13, color: 'var(--fg)', fontWeight: 500 }}>{p.title}</span>
                    {p.new && <Chip hot>NEW</Chip>}
                    {p.owned && <Chip solid>IN VAULT</Chip>}
                  </div>
                  <div className="row gap-8 nowrap" style={{ marginTop: 4, fontSize: 10, color: 'var(--fg-2)', overflow: 'hidden' }}>
                    <span className="truncate">{p.authors.join(', ')}</span>
                    <span style={{ color: 'var(--fg-3)' }}>·</span>
                    <span>{p.venue}</span>
                    <span style={{ color: 'var(--fg-3)' }}>·</span>
                    <span>cited {p.citations >= 1000 ? (p.citations/1000).toFixed(1)+'k' : p.citations}</span>
                  </div>
                  <div className="row gap-6 nowrap" style={{ marginTop: 8 }}>
                    <span className="mono-dim truncate" style={{ fontSize: 10, color: 'var(--amber-mid)' }}>↳ {p.why}</span>
                    <div className="flex1" />
                    {p.tags.map(t => <Chip key={t}>{t}</Chip>)}
                  </div>
                </div>
                <div className="col gap-6" style={{ width: 110 }}>
                  {p.owned ? (
                    <Btn ghost>▤ OPEN</Btn>
                  ) : (
                    <>
                      <Btn primary>＋ ADD</Btn>
                      <Btn ghost>▤ PREVIEW</Btn>
                      <Btn ghost>◇ GRAPH</Btn>
                    </>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </ShellA>
  );
};

// Search Console — the new inspector for Discover. Seeds, filters,
// signal mix, parallel agent count, schedule, batch action.
const InspectorSearchConsole = () => (
  <>
    <div className="hair-b" style={{ padding: '10px 14px', background: 'var(--bg-1)' }}>
      <div style={{ fontSize: 11, color: 'var(--amber)' }}>✦ SEARCH CONSOLE</div>
      <div className="mono-dim" style={{ fontSize: 10, marginTop: 2 }}>4 seeds · 3 agents · 8 candidates</div>
    </div>

    {/* Year range */}
    <div style={{ padding: '12px 14px 0' }}>
      <Label>YEAR RANGE</Label>
      <div className="row gap-6" style={{ marginTop: 6, alignItems: 'center' }}>
        <span className="mono-dim" style={{ fontSize: 10 }}>2018</span>
        <div style={{ flex: 1, height: 4, background: 'var(--bg-2)', border: '1px solid var(--border-2)', position: 'relative' }}>
          <div style={{ position: 'absolute', left: '32%', right: 0, top: 0, bottom: 0, background: 'var(--amber-dim)' }} />
          <div style={{ position: 'absolute', left: '32%', top: -3, bottom: -3, width: 1, background: 'var(--amber)' }} />
          <div style={{ position: 'absolute', right: 0, top: -3, bottom: -3, width: 1, background: 'var(--amber)' }} />
        </div>
        <span className="mono-dim" style={{ fontSize: 10 }}>2024</span>
      </div>
    </div>

    {/* Venues */}
    <div style={{ padding: '12px 14px 0' }}>
      <Label>VENUES</Label>
      <div className="row gap-4" style={{ marginTop: 6, flexWrap: 'wrap' }}>
        {['NeurIPS','ICML','ICLR','CVPR','ICCV','ECCV','TMLR','arXiv'].map((v, i) => <Chip key={v} hot={i < 5}>{v}</Chip>)}
      </div>
    </div>

    {/* Signal mix */}
    <div style={{ padding: '14px 14px 0' }}>
      <Label>SIGNAL MIX</Label>
      <div className="col gap-6" style={{ marginTop: 8, fontSize: 11 }}>
        {[
          ['semantic',   0.80],
          ['co-cite',    0.65],
          ['author net', 0.40],
          ['novelty',    0.55],
          ['venue tier', 0.30],
        ].map(([k, v]) => (
          <div key={k} className="row gap-8">
            <span style={{ width: 70, color: 'var(--fg-2)' }}>{k}</span>
            <Gauge value={v} w={70} />
            <span className="mono-dim" style={{ fontSize: 10 }}>{Math.round(v*100)}</span>
          </div>
        ))}
        <div className="mono-dim" style={{ fontSize: 9 }}>drag to retune ranking</div>
      </div>
    </div>

    {/* Agents */}
    <div style={{ padding: '14px 14px 0' }}>
      <Label>AGENTS · parallel search</Label>
      <div className="row gap-4" style={{ marginTop: 8 }}>
        {[1,2,3,4,5,6,7,8].map(n => (
          <div key={n} style={{
            flex: 1, height: 22, display: 'flex', alignItems: 'center', justifyContent: 'center',
            border: '1px solid ' + (n <= 3 ? 'var(--amber)' : 'var(--border-2)'),
            background: n <= 3 ? 'rgba(242,169,59,0.10)' : 'transparent',
            color: n <= 3 ? 'var(--amber)' : 'var(--fg-3)',
            fontSize: 10, fontWeight: 600,
          }}>{n}</div>
        ))}
      </div>
      <div className="row gap-6" style={{ marginTop: 6, fontSize: 10, color: 'var(--fg-2)' }}>
        <span style={{ color: 'var(--amber)' }}>●</span><span>semantic ⓘ</span>
        <span style={{ color: 'var(--amber)' }}>●</span><span>citation ⓘ</span>
        <span style={{ color: 'var(--amber)' }}>●</span><span>author ⓘ</span>
        <span style={{ color: 'var(--fg-3)' }}>·</span><span className="mono-dim">+5 idle</span>
      </div>
    </div>

    {/* Schedule */}
    <div style={{ padding: '14px 14px 0' }}>
      <Label>SCHEDULE</Label>
      <div className="col gap-3" style={{ marginTop: 8, fontSize: 11 }}>
        <ScheduleRow on label="On vault add"  sub="incremental — last run 12m ago" />
        <ScheduleRow on label="Daily 09:00"   sub="full re-rank — next in 6h" />
        <ScheduleRow    label="Weekly · Mon"  sub="deep search across arXiv" />
        <ScheduleRow    label="One-shot"      sub="run now once · ⌘⏎" />
      </div>
    </div>

    <div className="flex1" />

    <div className="hair-t" style={{ padding: '10px 14px', background: 'var(--bg-1)' }}>
      <Label>BATCH</Label>
      <div className="row gap-6" style={{ marginTop: 8 }}>
        <Btn primary>＋ ADD 5 STARRED</Btn>
        <Btn ghost>↓ .bib</Btn>
      </div>
    </div>
  </>
);

const ScheduleRow = ({ on, label, sub }) => (
  <div className="row gap-8" style={{
    padding: '5px 8px',
    border: '1px solid ' + (on ? 'var(--amber-dim)' : 'var(--border-2)'),
    background: on ? 'rgba(242,169,59,0.06)' : 'transparent',
  }}>
    <span style={{
      width: 12, height: 12, border: '1px solid ' + (on ? 'var(--amber)' : 'var(--border-2)'),
      background: on ? 'var(--amber)' : 'transparent', flexShrink: 0,
    }} />
    <div className="flex1" style={{ minWidth: 0 }}>
      <div style={{ color: on ? 'var(--amber)' : 'var(--fg-1)' }}>{label}</div>
      <div className="mono-dim" style={{ fontSize: 9, color: 'var(--fg-3)' }}>{sub}</div>
    </div>
  </div>
);

Object.assign(window, { ScreenA_Home, ScreenA_Reader, ScreenA_Reader_PDF, ScreenA_Discover });
