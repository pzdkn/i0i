// App entry — design canvas with 2 sections × 3 artboards + Tweaks.

const ARTBOARD_W = 1440;
const ARTBOARD_H = 900;

const PALETTES = {
  amber:    ['#f2a93b','#ffc452','#c98a32','#8c6620','#f0d9a8'],
  orange:   ['#e8853a','#ff9c46','#c46a26','#8c4920','#f0c9a4'],
  yellow:   ['#f3c83a','#ffdc52','#caa432','#8c7820','#f0e3a8'],
  phosphor: ['#8de054','#a7f562','#6cb53e','#48792a','#cfedb0'],
};
const PALETTE_OPTIONS = Object.values(PALETTES);

function paletteToCSS(p) {
  const [a, ab, am, ad, fg] = p;
  // derive fg shades from fg by reducing alpha visually (use direct mixes)
  return {
    '--amber': a,
    '--amber-bright': ab,
    '--amber-mid': am,
    '--amber-dim': ad,
    '--amber-faint': ad + '44',
    '--fg': fg,
    '--fg-1': fg + 'cc',
    '--fg-2': fg + '88',
    '--fg-3': fg + '55',
    '--fg-4': fg + '33',
    '--border':   ad + '38',
    '--border-2': ad + '70',
  };
}

function App() {
  const [tw, setTweak] = useTweaks(window.TWEAK_DEFAULTS);

  React.useEffect(() => {
    const css = paletteToCSS(tw.palette);
    const root = document.documentElement;
    Object.entries(css).forEach(([k, v]) => root.style.setProperty(k, v));
  }, [tw.palette]);

  return (
    <>
      <DesignCanvas>
        <DCSection id="layout-a" title="Layout A v2 · Refined IDE" subtitle="Mode rail · Explorer · Editor · Inspector — all panes collapsible. Reader has TEXT ↔ PDF. Discover uses an inline chip-pill seed with autocomplete; the inspector is now a Search Console (agents + schedule).">
          <DCArtboard id="a-home" label="A1 · Home — Vault folder" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenA_Home /></Tinted>
          </DCArtboard>
          <DCArtboard id="a-reader" label="A2 · Reader · TEXT — margin + inspector open" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenA_Reader /></Tinted>
          </DCArtboard>
          <DCArtboard id="a-reader-pdf" label="A3 · Reader · PDF — margin collapsed" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenA_Reader_PDF /></Tinted>
          </DCArtboard>
          <DCArtboard id="a-discover" label="A4 · Discover — Pill seed + Search Console" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenA_Discover /></Tinted>
          </DCArtboard>
        </DCSection>

        <DCSection id="layout-b" title="Layout B · Console / Cockpit (alt direction)" subtitle="Earlier exploration — kept here for reference. Same three screens with a heavier instrumentation feel.">
          <DCArtboard id="b-home" label="B1 · Home — Instrument panel" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenB_Home /></Tinted>
          </DCArtboard>
          <DCArtboard id="b-reader" label="B2 · Reader — Page + Agent + Lineage" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenB_Reader /></Tinted>
          </DCArtboard>
          <DCArtboard id="b-discover" label="B3 · Discover — Ranking console" width={ARTBOARD_W} height={ARTBOARD_H}>
            <Tinted scan={tw.scanlines} vig={tw.vignette}><ScreenB_Discover /></Tinted>
          </DCArtboard>
        </DCSection>
      </DesignCanvas>

      <TweaksPanel>
        <TweakSection label="Palette" />
        <TweakColor label="Hue" value={tw.palette} options={PALETTE_OPTIONS} onChange={(v) => setTweak('palette', v)} />
        <TweakSection label="Surface" />
        <TweakToggle label="Scanlines" value={tw.scanlines} onChange={(v) => setTweak('scanlines', v)} />
        <TweakToggle label="Vignette"  value={tw.vignette}  onChange={(v) => setTweak('vignette',  v)} />
      </TweaksPanel>
    </>
  );
}

// Tinted — overlay scanlines + vignette inside an artboard.
const Tinted = ({ children, scan, vig }) => (
  <div style={{ width: '100%', height: '100%', position: 'relative', overflow: 'hidden' }}>
    {children}
    {scan && <div style={{
      position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 60, opacity: 0.5,
      backgroundImage: 'repeating-linear-gradient(0deg, rgba(0,0,0,0) 0px, rgba(0,0,0,0) 2px, rgba(0,0,0,0.06) 3px, rgba(0,0,0,0.06) 3px)',
      mixBlendMode: 'multiply',
    }} />}
    {vig && <div style={{
      position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 61,
      background: 'radial-gradient(120% 80% at 50% 50%, transparent 55%, rgba(0,0,0,0.55) 100%)',
    }} />}
  </div>
);

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
