// Layout B — "Console / Cockpit"
// Horizontal mode tabs across top · slim vault on left · main pane · bottom command console
// Trades the right inspector for a fat bottom console with readouts and a live command line.
// Reads more like instrumentation than an IDE.

const ConsoleHeader = ({ active = 'V' }) => {
  const modes = [
    { k: 'V', label: 'VAULT'   },
    { k: 'F', label: 'FIND'    },
    { k: 'R', label: 'READ'    },
    { k: 'G', label: 'GRAPH'   },
    { k: 'A', label: 'ASK'     },
    { k: 'S', label: 'STUDY'   },
    { k: 'I', label: 'IMPORT'  },
  ];
  return (
    <div className="hair-b" style={{ height: 44, background: 'var(--bg-1)', display: 'flex', alignItems: 'stretch', flexShrink: 0 }}>
      {/* LOGO BLOCK */}
      <div className="hair-r" style={{ width: 92, padding: '0 10px', display: 'flex', flexDirection: 'column', justifyContent: 'center', gap: 1, background: 'var(--amber)', color: 'var(--bg)' }}>
        <div style={{ fontSize: 14, fontWeight: 700, letterSpacing: '0.02em' }}>1O1</div>
        <div style={{ fontSize: 8, letterSpacing: '0.18em' }}>RESEARCH UTILITIES</div>
      </div>

      {/* MODE TABS — square tiles like a hardware panel */}
      <div style={{ display: 'flex' }}>
        {modes.map(m => {
          const on = m.k === active;
          return (
            <div key={m.k} className="hair-r" style={{
              width: 72, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
              gap: 2,
              background: on ? 'var(--amber)' : 'transparent',
              color: on ? 'var(--bg)' : 'var(--fg-1)',
            }}>
              <span style={{ fontSize: 9, letterSpacing: '0.18em', fontWeight: 600 }}>{m.label}</span>
              <span style={{
                fontSize: 9, fontWeight: 700,
                border: '1px solid ' + (on ? 'var(--bg)' : 'var(--border-2)'),
                color: on ? 'var(--bg)' : 'var(--amber)',
                background: on ? 'rgba(0,0,0,0.08)' : 'transparent',
                padding: '0 4px', height: 12, display: 'inline-flex', alignItems: 'center',
              }}>{m.k}</span>
            </div>
          );
        })}
      </div>

      {/* PATH / CMD */}
      <div className="flex1 hair-l" style={{ display: 'flex', alignItems: 'center', gap: 14, padding: '0 12px', minWidth: 0 }}>
        <div className="row gap-6" style={{ fontSize: 11 }}>
          <span className="mono-dim" style={{ fontSize: 10 }}>PATH</span>
          <span style={{ color: 'var(--amber)' }}>~/vault</span>
          <span style={{ color: 'var(--fg-3)' }}>/</span>
          <span style={{ color: 'var(--fg-1)' }}>transformers</span>
          <span style={{ color: 'var(--fg-3)' }}>/</span>
          <span style={{ color: 'var(--fg)' }}>attention</span>
        </div>
        <div className="flex1" />
        <div className="row" style={{ border: '1px solid var(--border-2)', height: 26, padding: '0 10px', gap: 10, minWidth: 380, background: 'var(--bg)' }}>
          <span className="mono-dim" style={{ fontSize: 11, color: 'var(--amber)' }}>:</span>
          <span style={{ fontSize: 11, color: 'var(--fg-1)' }}>find sparse attention since 2022</span>
          <span style={{ width: 1, height: 14, background: 'var(--amber)', animation: 'none' }} />
          <div className="flex1" />
          <Key>⌘K</Key>
        </div>
      </div>

      {/* SYSTEM READOUT mini */}
      <div className="hair-l" style={{ width: 200, padding: '0 12px', display: 'flex', flexDirection: 'column', justifyContent: 'center', gap: 2, background: 'var(--bg-2)' }}>
        <div className="row gap-8" style={{ fontSize: 9, color: 'var(--fg-2)', letterSpacing: '0.12em' }}>
          <span>USR</span><span style={{ color: 'var(--amber)' }}>@gengeltje</span>
          <div className="flex1" />
          <span style={{ color: 'var(--green)' }}>●</span>
          <span>SYNC</span>
        </div>
        <div className="row gap-8" style={{ fontSize: 9, color: 'var(--fg-3)', letterSpacing: '0.12em' }}>
          <span>VLT</span><span style={{ color: 'var(--fg-1)' }}>234p · 12u · 87a</span>
        </div>
      </div>
    </div>
  );
};

const VaultPane = ({ width = 220 }) => (
  <div className="hair-r" style={{ width, flexShrink: 0, background: 'var(--panel)', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
    <div className="row hair-b" style={{ height: 24, padding: '0 10px', gap: 6, background: 'var(--bg-2)' }}>
      <Label hot>VAULT</Label>
      <div className="flex1" />
      <span className="mono-dim" style={{ fontSize: 10 }}>tree · 234p</span>
    </div>
    <div style={{ padding: '6px 8px' }}>
      <div className="row" style={{ border: '1px solid var(--border-2)', height: 22, padding: '0 6px', gap: 6, background: 'var(--bg)' }}>
        <span style={{ color: 'var(--amber)', fontSize: 11 }}>/</span>
        <span style={{ fontSize: 11, color: 'var(--fg-2)' }}>filter…</span>
        <div className="flex1" />
      </div>
    </div>
    <div className="flex1" style={{ overflow: 'auto' }}>
      <VaultTree compact />
    </div>
  </div>
);

const ConsolePane = () => (
  <div className="hair-t" style={{ height: 110, background: 'var(--panel)', display: 'flex', flexShrink: 0 }}>
    {/* Command line column */}
    <div className="flex1 col" style={{ minWidth: 0, padding: '8px 12px', borderRight: '1px solid var(--border)' }}>
      <Label hot style={{ marginBottom: 6 }}>CONSOLE · history 14 · session 02:48</Label>
      <div style={{ fontSize: 10.5, lineHeight: 1.55, color: 'var(--fg-2)', fontFamily: 'inherit' }}>
        <div><span style={{ color: 'var(--amber-mid)' }}>›</span> <span style={{ color: 'var(--fg-3)' }}>02:46:11</span> <span style={{ color: 'var(--amber)' }}>find</span> sparse attention since 2022 <span className="mono-dim">→ 47 results</span></div>
        <div><span style={{ color: 'var(--amber-mid)' }}>›</span> <span style={{ color: 'var(--fg-3)' }}>02:46:48</span> <span style={{ color: 'var(--amber)' }}>open</span> caron2021 <span className="mono-dim">→ ok</span></div>
        <div><span style={{ color: 'var(--amber-mid)' }}>›</span> <span style={{ color: 'var(--fg-3)' }}>02:47:02</span> <span style={{ color: 'var(--amber)' }}>ask</span> "what is the role of the centering trick?" <span style={{ color: 'var(--green)' }}>↩ 4 citations</span></div>
        <div><span style={{ color: 'var(--amber-mid)' }}>›</span> <span style={{ color: 'var(--fg-3)' }}>02:48:33</span> <span style={{ color: 'var(--amber)' }}>discover</span> --seed caron2021 --since 2022 <span className="mono-dim">→ 8 candidates</span></div>
      </div>
      <div className="row" style={{ marginTop: 6, border: '1px solid var(--amber-dim)', padding: '4px 8px', gap: 8, background: 'var(--bg)' }}>
        <span style={{ color: 'var(--amber)', fontWeight: 600 }}>1O1 :</span>
        <span style={{ fontSize: 11, color: 'var(--fg-1)' }}>add 5 starred to /self-supervised/dino-family</span>
        <span style={{ width: 6, height: 12, background: 'var(--amber)', display: 'inline-block' }} />
        <div className="flex1" />
        <Key>↵</Key>
      </div>
    </div>

    {/* Readouts cluster */}
    <div className="col" style={{ width: 360, padding: '8px 12px', gap: 6, borderRight: '1px solid var(--border)' }}>
      <Label hot>SYSTEM</Label>
      {[
        ['INDEX',  0.94, '94%', '11.2k chunks'],
        ['EMBED',  0.78, '78%', '234 / 300'],
        ['ASK',    0.42, '5/12', 'rate-limit'],
        ['SYNC',   1.00, '✓',   'just now'],
      ].map(([k, v, val, sub]) => (
        <div key={k} className="row gap-8" style={{ fontSize: 10 }}>
          <span style={{ width: 44, color: 'var(--fg-3)', letterSpacing: '0.1em' }}>{k}</span>
          <Gauge value={v} w={80} />
          <span style={{ width: 32, color: 'var(--amber)' }}>{val}</span>
          <span className="mono-dim" style={{ fontSize: 9 }}>{sub}</span>
        </div>
      ))}
    </div>

    {/* Hotkey legend */}
    <div className="col" style={{ width: 240, padding: '8px 12px', gap: 4 }}>
      <Label hot>HOTKEYS</Label>
      <div className="row gap-6" style={{ fontSize: 10 }}><Key>⌘K</Key><span className="mono-mid">command palette</span></div>
      <div className="row gap-6" style={{ fontSize: 10 }}><Key>/</Key><span className="mono-mid">filter / search</span></div>
      <div className="row gap-6" style={{ fontSize: 10 }}><Key>J</Key><Key>K</Key><span className="mono-mid">prev / next paper</span></div>
      <div className="row gap-6" style={{ fontSize: 10 }}><Key>O</Key><Key>↵</Key><span className="mono-mid">open</span></div>
      <div className="row gap-6" style={{ fontSize: 10 }}><Key>G</Key><Key>G</Key><span className="mono-mid">graph for selection</span></div>
    </div>
  </div>
);

const ShellB = ({ active = 'V', children, status }) => (
  <div className="crt" style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
    <ConsoleHeader active={active} />
    <div className="row flex1" style={{ minHeight: 0 }}>
      <VaultPane />
      <div className="flex1 col" style={{ minWidth: 0, background: 'var(--bg)' }}>
        {children}
      </div>
    </div>
    <ConsolePane />
    <StatusBar
      left={status?.left || [
        <>R/W ▤ <span style={{ color: 'var(--amber)' }}>vault.idx</span></>,
        <>234 papers · 12 unread</>,
        <>BibTeX export ready</>,
        <>cite-key: vaswani2017attention</>,
      ]}
      right={status?.right || [
        <>UTF-8</>,
        <>LF</>,
        <>EN</>,
        <>1O1 · v0.4.2</>,
        <span style={{ color: 'var(--green)' }}>● live</span>,
      ]}
    />
    <div className="crt-scanlines" />
    <div className="crt-vignette" />
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN B1 — HOME (Vault browser, grid card variant)
// ─────────────────────────────────────────────────────────────────────────
const ScreenB_Home = () => {
  const papers = window.PAPERS;
  return (
    <ShellB active="V">
      {/* Page header */}
      <div className="hair-b" style={{ padding: '12px 18px', background: 'var(--bg-1)' }}>
        <div className="row gap-8" style={{ marginBottom: 4 }}>
          <Label hot>FOLDER · /transformers/attention</Label>
        </div>
        <div className="row gap-14" style={{ alignItems: 'baseline' }}>
          <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700, color: 'var(--amber)', letterSpacing: '-0.01em' }}>attention</h1>
          <span className="mono-mid" style={{ fontSize: 11 }}>22 papers · 18 read · 2 reading · 2 unread</span>
          <div className="flex1" />
          <div className="row gap-6">
            <Btn primary>＋ ADD PAPER</Btn>
            <Btn>↑ IMPORT</Btn>
            <Btn>↓ EXPORT .BIB</Btn>
            <Btn>◇ GRAPH</Btn>
            <Btn ghost>⋯</Btn>
          </div>
        </div>

        {/* Strip of stats — instrument-panel feel */}
        <div className="row gap-0" style={{ marginTop: 12, border: '1px solid var(--border)', background: 'var(--bg-2)' }}>
          {[
            ['PAPERS',  '22',  '+3 / 7d', 0.95],
            ['NOTES',   '14',  '+5 / 7d', 0.55],
            ['ANN.',    '87',  '+12 / 7d', 0.75],
            ['CITES↑',  '142k','3 papers >50k', 1.0],
            ['SCOPE',   '2017—2024', '7 venues', 0.6],
            ['HEALTH',  '92%', '2 stale notes', 0.92],
          ].map(([k, v, sub, g], i) => (
            <div key={k} className="col" style={{ flex: 1, padding: '8px 12px', borderRight: i < 5 ? '1px solid var(--border)' : 'none' }}>
              <div className="row gap-6">
                <Label>{k}</Label>
              </div>
              <div style={{ fontSize: 16, color: 'var(--amber)', fontWeight: 600, marginTop: 2 }}>{v}</div>
              <div className="mono-dim" style={{ fontSize: 9, marginTop: 1 }}>{sub}</div>
              <Gauge value={g} w={'100%'} />
            </div>
          ))}
        </div>
      </div>

      {/* Toolbar */}
      <div className="row hair-b" style={{ height: 28, background: 'var(--bg-1)', padding: '0 18px', gap: 14, fontSize: 11, color: 'var(--fg-2)' }}>
        <Chip hot>● ALL 22</Chip>
        <Chip>foundational 8</Chip>
        <Chip>frontier 9</Chip>
        <Chip>survey 2</Chip>
        <Chip>to-implement 3</Chip>
        <div className="flex1" />
        <span className="mono-dim" style={{ fontSize: 10 }}>view ▤ list  /  ▦ grid  /  ◇ matrix</span>
        <span className="mono-dim" style={{ fontSize: 10 }}>sort: cites ↓</span>
      </div>

      {/* Body — card grid */}
      <div className="flex1" style={{ overflow: 'auto', padding: 14, display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10, alignContent: 'start' }}>
        {papers.map((p, i) => (
          <PaperCard key={p.id} p={p} featured={i === 0} />
        ))}
      </div>
    </ShellB>
  );
};

const PaperCard = ({ p, featured }) => (
  <div style={{
    border: '1px solid var(--border)',
    borderLeft: featured ? '2px solid var(--amber)' : '2px solid var(--border)',
    background: featured ? 'rgba(242,169,59,0.04)' : 'var(--bg-1)',
    padding: '10px 12px',
    display: 'flex', flexDirection: 'column', gap: 6,
    minHeight: 150,
  }}>
    <div className="row gap-6" style={{ fontSize: 9, letterSpacing: '0.1em', color: 'var(--fg-3)' }}>
      <span style={{ color: featured ? 'var(--amber)' : 'var(--fg-2)' }}>{p.year}</span>
      <span>·</span>
      <span>{p.venue}</span>
      <div className="flex1" />
      <span style={{ color: p.status === 'UNREAD' ? 'var(--amber)' : p.status === 'READING' ? 'var(--amber-bright)' : 'var(--fg-3)' }}>{p.status || '·'}</span>
    </div>
    <div style={{ fontSize: 12, color: 'var(--fg)', fontWeight: 500, lineHeight: 1.35, minHeight: 48 }}>{p.title}</div>
    <div className="mono-dim" style={{ fontSize: 10, color: 'var(--fg-3)' }} >{p.authors.slice(0,3).join(', ')}{p.authors.length > 3 ? ' +' + (p.authors.length - 3) : ''}</div>
    <div className="flex1" />
    <div className="row gap-4" style={{ fontSize: 9 }}>
      {(p.tags || []).slice(0, 3).map(t => <Chip key={t}>{t}</Chip>)}
    </div>
    <div className="row hair-t" style={{ paddingTop: 6, fontSize: 9, color: 'var(--fg-3)', gap: 10 }}>
      <span>cit {p.citations >= 1000 ? (p.citations/1000).toFixed(1)+'k' : p.citations}</span>
      <span>◆ {p.note || 0}</span>
      <span>⌥ {p.annotations || 0}</span>
      <div className="flex1" />
      <Key dim>O</Key>
    </div>
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN B2 — READER (panel grid: page · margin · agent · lineage)
// ─────────────────────────────────────────────────────────────────────────
const ScreenB_Reader = () => (
  <ShellB active="R">
    {/* Paper header — wide */}
    <div className="hair-b" style={{ padding: '12px 18px', background: 'var(--bg-1)' }}>
      <div className="row gap-8" style={{ marginBottom: 4 }}>
        <Label hot>READING · vault/transformers/attention</Label>
        <span className="mono-dim" style={{ fontSize: 10 }}>p.2 / 15 · §1 · 14m elapsed</span>
        <div className="flex1" />
        <Chip solid>READING</Chip>
        <Chip hot>foundational</Chip>
        <Chip>NeurIPS 2017</Chip>
      </div>
      <div className="row gap-14" style={{ alignItems: 'baseline' }}>
        <span style={{ fontSize: 9, letterSpacing: '0.2em', color: 'var(--amber-mid)' }}>arXiv:1706.03762</span>
        <h1 style={{ margin: 0, fontSize: 18, fontWeight: 600, color: 'var(--amber)' }}>Attention Is All You Need</h1>
        <span className="mono-mid" style={{ fontSize: 11 }}>Vaswani, Shazeer, Parmar, Uszkoreit, Jones, Gomez, Kaiser, Polosukhin</span>
        <div className="flex1" />
        <span className="mono-dim" style={{ fontSize: 10 }}>cit 134.8k · note 4 · ann 27</span>
      </div>
    </div>

    {/* Panel grid: 2 cols on left (page + margin), 1 col on right (agent + lineage stacked) */}
    <div className="row flex1" style={{ minHeight: 0 }}>
      {/* Page */}
      <div className="flex1" style={{ minWidth: 0, padding: '20px 30px', overflow: 'auto', background: 'var(--bg)' }}>
        <div style={{ maxWidth: 540, color: 'var(--fg)', fontSize: 12, lineHeight: 1.65 }}>
          <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.2em', marginBottom: 14 }}>§ 1 · INTRODUCTION ─────────────── p.2 / 15</div>
          {window.READER_BODY.map(b => {
            if (b.heading) {
              return <div key={b.id} style={{ fontWeight: 600, color: 'var(--amber)', margin: '20px 0 10px', fontSize: 12 }}>{b.heading}</div>;
            }
            const baseStyle = { margin: '0 0 12px 0' };
            if (b.highlight === 'amber') {
              return <p key={b.id} style={{ ...baseStyle, background: 'rgba(242,169,59,0.10)', borderLeft: '2px solid var(--amber)', padding: '4px 10px' }}>{b.text}</p>;
            }
            if (b.highlight === 'strong') {
              return <p key={b.id} style={{ ...baseStyle, background: 'rgba(242,169,59,0.18)', padding: '4px 10px', color: 'var(--amber-bright)' }}>{b.text}</p>;
            }
            return <p key={b.id} style={baseStyle}>{b.text}</p>;
          })}
        </div>
      </div>

      {/* Margin column */}
      <div className="hair-l" style={{ width: 240, background: 'var(--panel)', display: 'flex', flexDirection: 'column' }}>
        <div className="row hair-b" style={{ height: 24, padding: '0 10px', background: 'var(--bg-2)' }}>
          <Label hot>MARGIN</Label>
          <div className="flex1" />
          <span className="mono-dim" style={{ fontSize: 10 }}>§1 · 4 / 27</span>
        </div>
        <div style={{ padding: 10, fontSize: 10.5, lineHeight: 1.5, color: 'var(--fg-1)', overflow: 'auto' }}>
          <Annotation kind="note" anchor="¶3" body="Key motivation — RNN parallelism wall. See Chen 2018 §3 for follow-up." stamp="me · 2d" />
          <Annotation kind="question" anchor="¶5" body={"Q: how does this compare to ByteNet / ConvS2S in §2?"} stamp="me · 2d" cta="[A] ask vault" />
          <Annotation kind="hl" anchor="¶8" body={'"Self-attention … relating different positions of a single sequence" → tie to DINO\'s CLS-token attention maps.'} stamp="me · today" />
          <Annotation kind="ai" anchor="¶3" body="Cited by 12 papers in your vault to motivate sparse attention." stamp="1O1" />
        </div>
      </div>

      {/* Right column — Agent + Lineage stacked */}
      <div className="hair-l" style={{ width: 320, background: 'var(--panel)', display: 'flex', flexDirection: 'column', minHeight: 0 }}>
        {/* Agent panel */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
          <div className="row hair-b" style={{ height: 24, padding: '0 10px', background: 'var(--bg-2)' }}>
            <Label hot>AGENT · ASK THIS PAPER</Label>
            <div className="flex1" />
            <span className="mono-dim" style={{ fontSize: 10 }}>3 / 8</span>
          </div>
          <div style={{ flex: 1, overflow: 'auto', padding: 10, fontSize: 11, lineHeight: 1.55 }}>
            <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em', color: 'var(--amber)' }}>YOU · 02:46</div>
            <div style={{ color: 'var(--fg-1)', margin: '4px 0 10px' }}>What's the centering trick in DINO, and does this paper hint at why it works?</div>

            <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em', color: 'var(--amber)' }}>1O1 · cites Caron 2021 §3.2 · Vaswani 2017 ¶8</div>
            <div style={{ color: 'var(--fg-2)', margin: '4px 0 10px' }}>
              Centering keeps the teacher network's output from collapsing onto one dimension by subtracting a running mean
              over the batch <span style={{ color: 'var(--amber)' }}>[Caron 2021 §3.2]</span>. Vaswani 2017 doesn't address collapse directly but the
              softmax temperature scaling in eq.(1) is conceptually adjacent <span style={{ color: 'var(--amber)' }}>[¶8]</span>.
            </div>

            <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em', color: 'var(--amber)' }}>YOU · 02:47</div>
            <div style={{ color: 'var(--fg-1)', margin: '4px 0 10px' }}>Show me the actual eq.(1) and the temperature term.</div>

            <div className="mono-dim" style={{ fontSize: 9, letterSpacing: '0.1em', color: 'var(--amber)' }}>1O1 · cites ¶8 eq.(1)</div>
            <div style={{ color: 'var(--fg-2)', margin: '4px 0 4px' }}>
              <span style={{ background: 'rgba(242,169,59,0.10)', padding: '2px 4px' }}>Attention(Q,K,V) = softmax(QKᵀ / √dₖ) V</span>
              <span> — √dₖ is the temperature here.</span>
            </div>
          </div>
          <div className="hair-t row" style={{ padding: '6px 10px', gap: 6, background: 'var(--bg-1)' }}>
            <div className="flex1" style={{ border: '1px solid var(--border-2)', padding: '4px 6px', fontSize: 11, color: 'var(--fg-2)' }}>follow-up…</div>
            <Btn primary>↵</Btn>
          </div>
        </div>

        {/* Lineage panel */}
        <div className="hair-t" style={{ flexShrink: 0 }}>
          <div className="row hair-b" style={{ height: 24, padding: '0 10px', background: 'var(--bg-2)' }}>
            <Label hot>LINEAGE</Label>
            <div className="flex1" />
            <span className="mono-dim" style={{ fontSize: 10 }}>← 3  · → 5</span>
          </div>
          <LineageStrip />
        </div>
      </div>
    </div>
  </ShellB>
);

const Annotation = ({ kind, anchor, body, stamp, cta }) => {
  const map = {
    note:     { glyph: '◆ NOTE',      color: 'var(--amber)' },
    question: { glyph: '? QUESTION',  color: 'var(--amber-bright)' },
    hl:       { glyph: '▮ HIGHLIGHT', color: 'var(--amber-mid)' },
    ai:       { glyph: '✦ 1O1',       color: 'var(--green)' },
  };
  const m = map[kind];
  return (
    <div style={{ marginBottom: 10 }}>
      <div className="row gap-6" style={{ fontSize: 9, letterSpacing: '0.1em', marginBottom: 3 }}>
        <span style={{ color: m.color }}>{m.glyph} · {anchor}</span>
        <div className="flex1" />
        <span style={{ color: 'var(--fg-3)' }}>{stamp}</span>
      </div>
      <div style={{ border: '1px solid var(--border-2)', borderLeft: `2px solid ${m.color}`, padding: '5px 7px', color: kind === 'question' ? m.color : 'var(--fg-1)', whiteSpace: 'pre-wrap' }}>{body}</div>
      {cta && <div className="mono-dim" style={{ fontSize: 9, marginTop: 3 }}>{cta}</div>}
    </div>
  );
};

const LineageStrip = () => (
  <div style={{ padding: '8px 10px', fontSize: 10 }}>
    {/* Mini timeline */}
    <div style={{ position: 'relative', height: 56, marginBottom: 8 }}>
      {/* axis */}
      <div style={{ position: 'absolute', left: 0, right: 0, top: 28, height: 1, background: 'var(--border-2)' }} />
      {[
        { x: 0.04,  y: 2014, label: 'Bahdanau' },
        { x: 0.15,  y: 2015, label: 'Luong' },
        { x: 0.26,  y: 2016, label: 'Cheng' },
        { x: 0.40,  y: 2017, label: 'Vaswani', self: true },
        { x: 0.55,  y: 2019, label: 'BERT' },
        { x: 0.65,  y: 2019, label: 'GPT-2' },
        { x: 0.74,  y: 2020, label: 'GPT-3' },
        { x: 0.84,  y: 2021, label: 'ViT' },
        { x: 0.97,  y: 2023, label: 'LLaMA' },
      ].map((n, i) => (
        <div key={i} style={{ position: 'absolute', left: `${n.x * 100}%`, top: n.self ? 16 : 22, transform: 'translateX(-50%)' }}>
          <div style={{ width: n.self ? 12 : 6, height: n.self ? 12 : 6, background: n.self ? 'var(--amber)' : 'var(--amber-mid)', margin: '0 auto' }} />
          <div className="mono-dim" style={{ fontSize: 8, marginTop: 3, color: n.self ? 'var(--amber)' : 'var(--fg-3)', textAlign: 'center', whiteSpace: 'nowrap' }}>{n.label}</div>
          <div className="mono-dim" style={{ fontSize: 8, color: 'var(--fg-4)', textAlign: 'center' }}>{n.y}</div>
        </div>
      ))}
    </div>
    <div className="row gap-8" style={{ fontSize: 9, color: 'var(--fg-3)' }}>
      <span style={{ color: 'var(--amber)' }}>● in vault</span>
      <span style={{ color: 'var(--amber-mid)' }}>◆ related</span>
      <div className="flex1" />
      <span className="mono-dim">[G] expand graph</span>
    </div>
  </div>
);

// ─────────────────────────────────────────────────────────────────────────
// SCREEN B3 — DISCOVER (instrument-panel style)
// ─────────────────────────────────────────────────────────────────────────
const ScreenB_Discover = () => (
  <ShellB active="F">
    {/* Header */}
    <div className="hair-b" style={{ padding: '12px 18px', background: 'var(--bg-1)' }}>
      <div className="row gap-8" style={{ marginBottom: 4 }}>
        <Label hot>DISCOVER · seed-based ranking</Label>
      </div>
      <div className="row gap-14" style={{ alignItems: 'baseline' }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 600, color: 'var(--amber)' }}>find papers like…</h1>
        <span className="mono-mid" style={{ fontSize: 11 }}>
          seed: <span style={{ color: 'var(--amber)' }}>caron2021</span>
          <span style={{ color: 'var(--fg-3)' }}> + </span>
          <span style={{ color: 'var(--amber)' }}>/self-supervised</span>
          <span style={{ color: 'var(--fg-3)' }}> + </span>
          tags: <span style={{ color: 'var(--amber)' }}>frontier</span>
        </span>
        <div className="flex1" />
        <Btn primary>＋ ADD STARRED 5</Btn>
        <Btn>↺ rerank</Btn>
      </div>
    </div>

    {/* Two-column main */}
    <div className="row flex1" style={{ minHeight: 0 }}>
      {/* Candidates list */}
      <div className="flex1" style={{ minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <div className="row hair-b" style={{ height: 26, padding: '0 14px', background: 'var(--bg-1)', gap: 14, fontSize: 9, letterSpacing: '0.12em', color: 'var(--fg-3)', textTransform: 'uppercase' }}>
          <span style={{ width: 40 }}>SCORE</span>
          <span style={{ width: 36 }}>YEAR</span>
          <span style={{ width: 56 }}>VENUE</span>
          <span className="flex1">TITLE · WHY</span>
          <span style={{ width: 60, textAlign: 'right' }}>CITES</span>
          <span style={{ width: 100, textAlign: 'right' }}>SIGNALS</span>
          <span style={{ width: 80, textAlign: 'right' }}>ACTION</span>
        </div>
        <div className="flex1" style={{ overflow: 'auto' }}>
          {window.DISCOVER_FEED.map(p => (
            <div key={p.id} className="row" style={{
              padding: '10px 14px', gap: 14, borderBottom: '1px solid var(--border)',
              background: p.owned ? 'rgba(138,168,74,0.05)' : 'transparent',
              borderLeft: p.owned ? '2px solid var(--green)' : '2px solid transparent',
              alignItems: 'flex-start',
            }}>
              <span style={{ width: 40, fontSize: 14, fontWeight: 700, color: 'var(--amber)' }}>{p.score.toFixed(2)}</span>
              <span style={{ width: 36, fontSize: 10, color: 'var(--fg-2)' }}>{p.venue.match(/\d{4}/)?.[0]}</span>
              <span style={{ width: 56, fontSize: 10, color: 'var(--fg-2)' }}>{p.venue.split(' ·')[0]}</span>
              <div className="flex1" style={{ minWidth: 0 }}>
                <div className="row gap-6" style={{ alignItems: 'baseline' }}>
                  <span style={{ fontSize: 12, color: 'var(--fg)', fontWeight: 500 }}>{p.title}</span>
                  {p.new && <Chip hot>NEW</Chip>}
                  {p.owned && <Chip solid>IN VAULT</Chip>}
                </div>
                <div className="mono-dim" style={{ fontSize: 10, marginTop: 2 }}>{p.authors.slice(0,3).join(', ')}{p.authors.length > 3 ? ' +' + (p.authors.length - 3) : ''}</div>
                <div className="mono-dim" style={{ fontSize: 10, marginTop: 4, color: 'var(--amber-mid)' }}>↳ {p.why}</div>
              </div>
              <span style={{ width: 60, fontSize: 10, color: 'var(--fg-3)', textAlign: 'right' }}>{p.citations >= 1000 ? (p.citations/1000).toFixed(1)+'k' : p.citations}</span>
              <div style={{ width: 100, textAlign: 'right' }}>
                <SignalBars score={p.score} />
              </div>
              <div style={{ width: 80, textAlign: 'right' }}>
                {p.owned ? <Btn ghost>▤ OPEN</Btn> : <Btn primary>＋ ADD</Btn>}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Filters / controls — right column, dial-cluster style */}
      <div className="hair-l" style={{ width: 260, background: 'var(--panel)', display: 'flex', flexDirection: 'column', overflow: 'auto' }}>
        <div className="row hair-b" style={{ height: 24, padding: '0 10px', background: 'var(--bg-2)' }}>
          <Label hot>FILTERS / TUNING</Label>
        </div>

        <div style={{ padding: '12px 12px 0' }}>
          <Label>YEAR RANGE</Label>
          <div className="row gap-6" style={{ marginTop: 6, alignItems: 'center' }}>
            <span className="mono-dim" style={{ fontSize: 10 }}>2018</span>
            <div style={{ flex: 1, height: 4, background: 'var(--bg-2)', border: '1px solid var(--border-2)', position: 'relative' }}>
              <div style={{ position: 'absolute', left: '20%', right: 0, top: 0, bottom: 0, background: 'var(--amber-dim)' }} />
              <div style={{ position: 'absolute', left: '20%', top: -3, bottom: -3, width: 1, background: 'var(--amber)' }} />
              <div style={{ position: 'absolute', right: 0, top: -3, bottom: -3, width: 1, background: 'var(--amber)' }} />
            </div>
            <span className="mono-dim" style={{ fontSize: 10 }}>2024</span>
          </div>
        </div>

        <div style={{ padding: '12px 12px 0' }}>
          <Label>VENUES</Label>
          <div className="row gap-4" style={{ marginTop: 6, flexWrap: 'wrap' }}>
            {['NeurIPS','ICML','ICLR','CVPR','ICCV','ECCV','TMLR','arXiv'].map((v, i) => <Chip key={v} hot={i < 5}>{v}</Chip>)}
          </div>
        </div>

        <div style={{ padding: '14px 12px 0' }}>
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
                <span style={{ width: 76, color: 'var(--fg-2)' }}>{k}</span>
                <Gauge value={v} w={70} />
                <span className="mono-dim" style={{ fontSize: 10 }}>{Math.round(v*100)}</span>
              </div>
            ))}
          </div>
        </div>

        <div style={{ padding: '14px 12px 0' }}>
          <Label>EXCLUDED</Label>
          <div className="col gap-4" style={{ marginTop: 8, fontSize: 10.5, color: 'var(--fg-2)' }}>
            <div className="row gap-6"><span style={{ color: 'var(--fg-3)' }}>·</span><span className="flex1">already in vault</span><span className="mono-dim">23</span></div>
            <div className="row gap-6"><span style={{ color: 'var(--fg-3)' }}>·</span><span className="flex1">pre-2018</span><span className="mono-dim">412</span></div>
            <div className="row gap-6"><span style={{ color: 'var(--fg-3)' }}>·</span><span className="flex1">dismissed</span><span className="mono-dim">7</span></div>
          </div>
        </div>

        <div className="flex1" />

        <div className="hair-t" style={{ padding: '10px 12px', background: 'var(--bg-1)' }}>
          <Label>BATCH</Label>
          <div className="row gap-6" style={{ marginTop: 8 }}>
            <Btn primary>＋ ADD 5</Btn>
            <Btn ghost>↓ .bib</Btn>
          </div>
        </div>
      </div>
    </div>
  </ShellB>
);

const SignalBars = ({ score }) => {
  // Mini 5-bar instrument readout per candidate
  const seed = Math.floor(score * 100);
  const bars = [0,1,2,3,4].map(i => ((seed + i * 17) % 100) / 100);
  return (
    <div className="row" style={{ gap: 2, justifyContent: 'flex-end' }}>
      {bars.map((b, i) => (
        <div key={i} style={{ width: 4, height: 16, background: 'var(--bg-2)', border: '1px solid var(--border-2)', position: 'relative' }}>
          <div style={{ position: 'absolute', bottom: 0, left: 0, right: 0, background: 'var(--amber)', height: `${Math.round(b * 100)}%` }} />
        </div>
      ))}
    </div>
  );
};

Object.assign(window, { ScreenB_Home, ScreenB_Reader, ScreenB_Discover });
