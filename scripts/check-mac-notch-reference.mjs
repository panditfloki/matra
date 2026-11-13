#!/usr/bin/env node
// A deliberately narrow Swift-source reader, not a Swift interpreter. Unknown
// syntax is an error; source values never fall back to last generated values.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile, realpath } from 'node:fs/promises';
import { dirname, resolve, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const OUTPUT = 'shared/notch-reference/mac-notch-reference.json';
const RUNTIME = 'native/matra/ui/matra-notch-reference.js';
const SOURCE = {
  design: 'native/macos/Sources/DesignSystem/Design.swift',
  layout: 'native/macos/Sources/Notch/NotchLayout.swift',
  edge: 'native/macos/Sources/Notch/NotchEdge.swift',
  motion: 'native/macos/Sources/Notch/NotchMotion.swift',
  controller: 'native/macos/Sources/Notch/NotchWindowController.swift',
  rootView: 'native/macos/Sources/Notch/NotchRootView.swift',
  size: 'native/macos/Sources/Settings/NotchSize.swift',
  shape: 'native/macos/Sources/Notch/SideNotchShape.swift',
  settingsHandle: 'native/macos/Sources/Features/SettingsHandle.swift',
  moveHandle: 'native/macos/Sources/Features/MoveHandle.swift',
  providerRing: 'native/macos/Sources/Features/ProviderRing.swift',
  layoutTests: 'native/macos/Tests/NotchLayoutTests.swift',
  edgeTests: 'native/macos/Tests/NotchEdgeTests.swift',
};
const LENGTHS = `sideBodyDepth curlRadius bezelFillet cornerRadius padTop padBottom
cellSpacing pillWidth pillHeight pillHotZone ringDiameter splitCornerRadius
trackStroke progressStroke glyphSize ringLabelGap activityDiameter activityStroke
weeklyRingStroke weeklyInsideRadius weeklyOutsideRadius orbDiameter orbStroke orbGap
orbClearance orbGlyph orbHotZone gripDot gripPitch gripWidth gripLength gripGap
gripHotZone cardWidth updateCardWidth cardCorner cardPadding tailLength tailHeight
tailGap barHeight headerGap headerToBlock labelToBar barToUsed blockSpacing
moneyBarHeight moneyBarToStats moneyStatGap usageDetailIdentityGap
usageDetailBarHeight usageDetailLabelToBar usageDetailBarToStats
usageDetailChartHeight usageDetailChartGap usageDetailBarGap sessionRowGap
statusDot statusDotStroke statusDotGap hairline codexUsageTop codexMetricTop
codexMetricRowGap codexMetricRowHeight codexMetricHeight codexMetricBottom
codexUsageRowGap codexChartTop codexChartHeight splitRowGap splitRowSlack endSlack`
  .split(/\s+/);
const GETTERS = ['sideRingMargin', 'orbArcRadius', 'orbMergeScale', 'cardTextWidth', 'orbInsetFromEdge'];
const COUNTS = ['maxWindowCount', 'sessionCeiling', 'defaultSessionCap'];
const EDGES = ['right', 'left', 'top', 'bottom'];
const escapeRE = text => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const uncomment = source => source.replace(/\/\/[^\n]*/g, '');
const compact = source => uncomment(source).replace(/\s+/g, ' ').trim();
const serialize = value => JSON.stringify(value, null, 2) + '\n';
const digest = source => createHash('sha256').update(source).digest('hex');

function one(source, pattern, label) {
  const matches = [...source.matchAll(new RegExp(pattern.source, pattern.flags.replace(/g/g, '') + 'g'))];
  assert.equal(matches.length, 1, `${label}: expected one match, found ${matches.length}`);
  return matches[0];
}

function block(source, pattern, label) {
  source = uncomment(source);
  const match = one(source, pattern, label);
  const start = source.indexOf('{', match.index + match[0].length);
  assert(start >= 0, `${label}: missing body`);
  let level = 1;
  let end = start + 1;
  for (; end < source.length && level; end++) {
    if (source[end] === '{') level++;
    if (source[end] === '}') level--;
  }
  assert.equal(level, 0, `${label}: unterminated body`);
  return source.slice(start + 1, end - 1).trim();
}

function declaration(source, name, kind = 'let', isStatic = true) {
  const keyword = kind === 'initializedVar' ? 'var' : kind;
  const pattern = new RegExp(`^[ \\t]*(?:private[ \\t]+)?${isStatic ? 'static[ \\t]+' : ''}${keyword}[ \\t]+${escapeRE(name)}\\b[^\\n]*$`, 'm');
  const stripped = uncomment(source);
  const match = one(stripped, pattern, name);
  const line = match[0].trim();
  const next = stripped.slice(match.index + match[0].length).trimStart().split('\n')[0];
  assert(!next || /^(?:(?:private|fileprivate)\s+)?(?:static\s+)?(?:let|var|func|enum|struct|class)\b|^[}@]/.test(next), `${name}: unsupported continuation ${next}`);
  const rhs = kind !== 'var'
    ? line.match(/\b(?:let|var)\s+\w+\s*(?::\s*\w+)?\s*=\s*(.+)$/)?.[1]
    : line.match(/\bvar\s+\w+\s*:\s*\w+\s*\{\s*(.+)\s*\}$/)?.[1];
  assert(rhs, `${name}: unsupported declaration ${line}`);
  return rhs.trim();
}

// Only finite decimal numbers, known names, arithmetic, and Design.px(...) are
// accepted. In particular, there is no eval/Function or unknown-name default.
export function numberExpression(expression, values = {}, designScale) {
  const tokens = [];
  let offset = 0;
  while (offset < expression.length) {
    const match = /^(\s+|\d+(?:\.\d+)?|[A-Za-z_][A-Za-z_0-9]*(?:\.[A-Za-z_][A-Za-z_0-9]*)?|[()+*/-])/.exec(expression.slice(offset));
    assert(match, `unsupported expression: ${expression}`);
    if (!/^\s+$/.test(match[0])) tokens.push(match[0]);
    offset += match[0].length;
  }
  let at = 0;
  const take = () => tokens[at++];
  function primary() {
    const token = take();
    if (token === '-' || token === '+') return (token === '-' ? -1 : 1) * primary();
    if (token === '(') {
      const value = sum();
      assert.equal(take(), ')', `unclosed expression: ${expression}`);
      return value;
    }
    if (/^\d+(?:\.\d+)?$/.test(token ?? '')) return Number(token);
    if (token === 'Design.px') {
      assert(Number.isFinite(designScale), 'Design.px requires a verified design scale');
      assert.equal(take(), '(');
      const value = sum();
      assert.equal(take(), ')');
      return value * designScale;
    }
    assert(Object.hasOwn(values, token), `unknown name ${token} in ${expression}`);
    return values[token];
  }
  function product() {
    let value = primary();
    while (tokens[at] === '*' || tokens[at] === '/') {
      const op = take();
      const rhs = primary();
      value = op === '*' ? value * rhs : value / rhs;
    }
    return value;
  }
  function sum() {
    let value = product();
    while (tokens[at] === '+' || tokens[at] === '-') {
      const op = take();
      const rhs = product();
      value = op === '+' ? value + rhs : value - rhs;
    }
    return value;
  }
  const result = sum();
  assert.equal(at, tokens.length, `unconsumed expression: ${expression}`);
  assert(Number.isFinite(result), `non-finite expression: ${expression}`);
  return result;
}

function switchCases(body, subject, label) {
  const source = compact(body);
  const prefix = `switch ${subject} { `;
  assert(source.startsWith(prefix) && source.endsWith(' }'), `${label}: unsupported switch`);
  const contents = source.slice(prefix.length, -2);
  const matches = [...contents.matchAll(/case \.(\w+):\s*return\s+(.+?)(?= case \.|$)/g)];
  assert.equal(matches.map(m => m[0]).join(' '), contents, `${label}: unsupported case expression`);
  const result = Object.fromEntries(matches.map(m => [m[1], m[2]]));
  assert.equal(Object.keys(result).length, matches.length, `${label}: duplicate case`);
  return result;
}

function point(expression) {
  const match = /^CGPoint\(x:\s*([^,]+),\s*y:\s*([^,)]+)\)$/.exec(expression);
  assert(match, `unsupported point: ${expression}`);
  return { x: numberExpression(match[1]), y: numberExpression(match[2]) };
}

export function generateReference(sources) {
  const get = key => {
    assert.equal(typeof sources[key], 'string', `missing source: ${key}`);
    return sources[key];
  };
  const designSource = get('design');
  assert.equal(compact(block(designSource, /static func px\(_ pixels: CGFloat\) -> CGFloat/, 'Design.px')), 'pixels * scale', 'unsupported Design.px conversion');
  assert.equal(compact(block(designSource, /static func fontSize\(capPixels pixels: CGFloat\) -> CGFloat/, 'Design.fontSize')), 'px(pixels) / capRatio', 'unsupported font conversion');
  const scale = numberExpression(declaration(designSource, 'scale'));
  const capRatio = numberExpression(declaration(designSource, 'capRatio'));
  assert(scale > 0 && capRatio > 0 && capRatio <= 1, 'invalid design scale or cap ratio');
  const layoutSource = get('layout');
  const layout = {};
  const designPixels = {};
  for (const name of [...LENGTHS, 'localArcMinimumSweep', ...COUNTS, ...GETTERS]) {
    const expression = declaration(layoutSource, name, GETTERS.includes(name) ? 'var' : 'let');
    layout[name] = numberExpression(expression, layout, scale);
    assert(layout[name] > 0, `non-positive layout value: ${name}`);
    const designMatch = /^Design\.px\((.+)\)$/.exec(expression);
    if (designMatch) designPixels[name] = numberExpression(designMatch[1]);
  }
  const typography = {};
  for (const [name, pattern] of [
    ['percentLabel', /static let percentLineHeight: CGFloat = \{\s*let font = NSFont\.systemFont\(ofSize: Design\.fontSize\(capPixels: ([\d.]+)\), weight: \.semibold\)\s*return ceil\(font\.ascender - font\.descender \+ font\.leading\)\s*\}\(\)/],
    ['cardTitle', /static let cardTitleLineHeight: CGFloat = lineHeight\(\s*NSFont\.systemFont\(ofSize: Design\.fontSize\(capPixels: ([\d.]+)\), weight: \.semibold\)\s*\)/],
    ['cardBody', /static let cardBodyFont = NSFont\.systemFont\(\s*ofSize: Design\.fontSize\(capPixels: ([\d.]+)\), weight: \.regular\s*\)/],
  ]) {
    const capDesignPixels = numberExpression(one(uncomment(layoutSource), pattern, name)[1]);
    typography[name] = { capDesignPixels, fontSize: capDesignPixels * scale / capRatio };
  }
  const edgeSource = get('edge');
  const verticalExpression = compact(block(edgeSource, /var isVertical: Bool/, 'isVertical'));
  assert(/^self == \.[a-z]+(?: \|\| self == \.[a-z]+)*$/.test(verticalExpression), 'unsupported vertical-edge expression');
  const verticalEdges = [...verticalExpression.matchAll(/self == \.([a-z]+)/g)].map(m => m[1]);
  assert(verticalEdges.every(edge => EDGES.includes(edge)), 'unknown vertical edge');
  const directions = switchCases(block(edgeSource, /var tooltipDirection: TooltipDirection/, 'tooltipDirection'), 'self', 'tooltipDirection');
  const outward = switchCases(block(edgeSource, /var outward: CGPoint/, 'outward'), 'self', 'outward');
  const along = compact(block(edgeSource, /var alongDirection: CGPoint/, 'alongDirection'));
  const alongMatch = /^isVertical \? (CGPoint\(.+?\)) : (CGPoint\(.+?\))$/.exec(along);
  assert(alongMatch, 'unsupported alongDirection expression');
  const bezel = switchCases(block(get('rootView'), /private var bezelAnchor: UnitPoint/, 'bezelAnchor'), 'model.edge', 'bezelAnchor');
  for (const cases of [directions, outward, bezel]) assert.deepEqual(Object.keys(cases).sort(), [...EDGES].sort());
  const anchors = Object.fromEntries(EDGES.map(edge => {
    assert(/^\.(leading|trailing|up|down)$/.test(directions[edge]), 'unknown tooltip direction');
    assert(/^\.(leading|trailing|top|bottom)$/.test(bezel[edge]), 'unknown bezel anchor');
    return [edge, {
      isVertical: verticalEdges.includes(edge),
      tooltipDirection: directions[edge].slice(1),
      outward: point(outward[edge]),
      along: point(alongMatch[verticalEdges.includes(edge) ? 1 : 2]),
      bezelAnchor: bezel[edge].slice(1),
    }];
  }));
  const motionSource = get('motion');
  const motion = {};
  for (const name of ['unfold', 'lift', 'contents', 'glide', 'crossfade', 'reading', 'merge']) {
    const expression = declaration(motionSource, name);
    const spring = /^Animation\.spring\(response:\s*([\d.]+),\s*dampingFraction:\s*([\d.]+)\)$/.exec(expression);
    const ease = /^Animation\.(easeInOut|easeIn)\(duration:\s*([\d.]+)\)$/.exec(expression);
    assert(spring || ease, `unsupported animation: ${name}`);
    motion[name] = spring
      ? { kind: 'spring', responseSeconds: numberExpression(spring[1]), dampingFraction: numberExpression(spring[2]) }
      : { kind: ease[1], durationSeconds: numberExpression(ease[2]) };
  }
  const stagger = compact(block(motionSource, /static func stagger\(index: Int\) -> Animation/, 'stagger'));
  const staggerMatch = /^contents\.delay\(min\(Double\(index\) \* ([\d.]+), ([\d.]+)\)\)$/.exec(stagger);
  assert(staggerMatch, 'unsupported stagger expression');
  motion.stagger = { stepSeconds: numberExpression(staggerMatch[1]), capSeconds: numberExpression(staggerMatch[2]) };
  assert.equal(compact(block(motionSource, /static func respectingReduceMotion\(_ animation: Animation, _ reduce: Bool\) -> Animation\?/, 'reduceMotion')), 'reduce ? nil : animation', 'unsupported reduce-motion behavior');
  motion.reduceMotion = 'none';
  const sizes = Object.fromEntries(Object.entries(switchCases(block(get('size'), /var scale: CGFloat/, 'NotchSize.scale'), 'self', 'NotchSize.scale')).map(([key, value]) => [key, numberExpression(value)]));
  assert.deepEqual(Object.keys(sizes).sort(), ['large', 'medium', 'small']);
  const shapeSource = get('shape');
  const fluidTurn = block(shapeSource, /private func fluidTurn\(_ path: inout Path, to: CGPoint,\s*leaving: CGVector, arriving: CGVector, ramp: CGFloat\)/, 'fluidTurn');
  const shape = {
    circleReach: numberExpression(declaration(shapeSource, 'circleReach')),
    filletRamp: numberExpression(declaration(shapeSource, 'filletRamp', 'initializedVar', false)),
    fluidTurnSteps: numberExpression(declaration(fluidTurn, 'steps', 'let', false)),
  };
  assert(shape.circleReach > 0 && shape.circleReach < 1);
  assert(shape.filletRamp >= 0 && shape.filletRamp <= 0.5);
  assert(Number.isSafeInteger(shape.fluidTurnSteps) && shape.fluidTurnSteps > 0);
  const ringSource = uncomment(get('providerRing'));
  const pair = (pattern, label) => one(ringSource, pattern, label).slice(1).map(value => numberExpression(value));
  const [staleReduced, stale] = pair(/\.opacity\(isStale \? \(reduceTransparency \? ([\d.]+) : ([\d.]+)\) : 1\)/, 'stale opacity');
  const [spentReduced, spent] = pair(/\.opacity\(band == \.exhausted \? \(reduceTransparency \? ([\d.]+) : ([\d.]+)\) : 1\)/, 'exhausted glyph opacity');
  const unknownDash = one(ringSource, /dash: usedFraction\?\.isFinite == true \|\| localPerformance != nil \|\| localContextFraction != nil \? \[\] : \[([^\]]+)\]\)\)/, 'unknown track dash')[1];
  const weeklyTrack = one(ringSource, /\.stroke\(Palette\.ringTrack,\s*style: StrokeStyle\(lineWidth: NotchLayout\.weeklyRingStroke,\s*dash: weeklyRingDashed \? \[([^\]]+)\] : \[\]\)\)\s*\.opacity\(reduceTransparency \? ([\d.]+) : ([\d.]+)\)/, 'weekly track');
  const weeklyArc = one(ringSource, /\.stroke\(\s*weeklyRingColor,\s*style: StrokeStyle\(lineWidth: NotchLayout\.weeklyRingStroke,\s*lineCap: weeklyRingDashed \? \.butt : \.round,\s*dash: weeklyRingDashed \? \[([^\]]+)\] : \[\]\)\s*\)\s*\.opacity\(reduceTransparency \? ([\d.]+) : ([\d.]+)\)/, 'weekly arc');
  const dashArray = expression => expression.split(',').map(value => numberExpression(value));
  assert.deepEqual(dashArray(weeklyTrack[1]), dashArray(weeklyArc[1]), 'weekly track and arc dash differ');
  const cells = block(get('rootView'), /private var cells: some View/, 'provider cells');
  const retreat = one(cells, /\.offset\(\s*x: model\.isExpanded \? 0 : model\.edge\.outward\.x \* Design\.px\(([\d.]+)\),\s*y: model\.isExpanded \? 0 : model\.edge\.outward\.y \* Design\.px\(([\d.]+)\)\s*\)/, 'content retreat').slice(1).map(value => numberExpression(value));
  assert.equal(retreat[0], retreat[1], 'content retreat differs by axis');
  const transitionRetreat = one(cells, /\.transition\(\.opacity\.combined\(with: \.offset\(\s*x: model\.edge\.outward\.x \* Design\.px\(([\d.]+)\),\s*y: model\.edge\.outward\.y \* Design\.px\(([\d.]+)\)\s*\)\)/, 'content transition retreat').slice(1).map(value => numberExpression(value));
  assert.deepEqual(transitionRetreat, retreat, 'content transition and fold retreat differ');
  const rendering = {
    bezelBleed: numberExpression(declaration(get('rootView'), 'bezelBleed')),
    contentRetreatDesignPixels: retreat[0], contentRetreat: retreat[0] * scale,
    unknownTrackDash: dashArray(unknownDash), weeklyDash: dashArray(weeklyTrack[1]),
    activityArcFraction: numberExpression(declaration(block(ringSource, /private struct ActivityArc: View/, 'ActivityArc'), 'arcFraction', 'let', false)),
    staleOpacity: stale, staleReducedTransparencyOpacity: staleReduced,
    exhaustedGlyphOpacity: spent, exhaustedGlyphReducedTransparencyOpacity: spentReduced,
    weeklyTrackOpacity: numberExpression(weeklyTrack[3]), weeklyTrackReducedTransparencyOpacity: numberExpression(weeklyTrack[2]),
    weeklyArcOpacity: numberExpression(weeklyArc[3]), weeklyArcReducedTransparencyOpacity: numberExpression(weeklyArc[2]),
  };
  for (const [name, value] of Object.entries(rendering)) {
    if (/Opacity$|Fraction$/.test(name)) assert(value >= 0 && value <= 1, `invalid rendering fraction: ${name}`);
    if (Array.isArray(value)) assert(value.length === 2 && value.every(n => n > 0), `invalid dash: ${name}`);
  }
  const reference = {
    schemaVersion: 1,
    kind: 'mac-notch-reference-data',
    units: {
      layout: 'logical-point',
      layoutExceptions: { localArcMinimumSweep: 'fraction-of-circle', orbMergeScale: 'scale-factor', maxWindowCount: 'count', sessionCeiling: 'count', defaultSessionCap: 'count' },
      designPixels: 'reference-frame-pixel', time: 'second', axes: 'top-left-origin-y-down',
    },
    design: { pointsPerDesignPixel: scale, sfProCapRatio: capRatio },
    designPixels,
    layout,
    typography,
    motion,
    shape,
    rendering,
    interaction: {
      hoverGraceSeconds: numberExpression(declaration(get('controller'), 'hoverGrace', 'let', false)),
      foldGraceSeconds: numberExpression(declaration(get('controller'), 'foldGrace', 'let', false)),
      settingsTurnBackSeconds: numberExpression(declaration(get('settingsHandle'), 'turnsBack')),
      gripGrowth: numberExpression(declaration(get('moveHandle'), 'hoverGrowth')),
    },
    sizes,
    anchors,
    nativeMeasured: ['percentLineHeight', 'cardTitleLineHeight', 'cardBodyLineHeight', 'wrappedTextHeight', 'screenAndCutoutGeometry'],
    provenance: {
      authority: 'Existing native Mac source; this generated data does not replace its presentation or runtime.',
      generator: 'scripts/check-mac-notch-reference.mjs',
      sources: Object.entries(SOURCE).map(([key, path]) => ({ path, sha256: digest(get(key)) })),
    },
  };
  validateExpectations(reference, sources);
  return reference;
}

// These independent constraints are transcribed from existing Mac tests, not
// calculated by re-running the extraction expressions. Native test names are
// required to remain present and their full files are fingerprinted above.
function validateExpectations(reference, sources) {
  for (const name of ['testRingIsTheSpecAnchor', 'testProportionsMatchTheFrame',
    'testActivityRingClearsTheGlyphAndTheTrack', 'testTheInsideWeeklyRingClearsTheGlyphAndTheWorkingIndicator',
    'testTheOutsideWeeklyRingClearsTheTrackAndTheBezel', 'testTheWeeklyRingIsThinnerThanTheHeadline',
    'testTheMinimumArcIsLongerThanItsCaps', 'testTheStaggerIsBounded', 'testReduceMotionRemovesTheAnimation']) {
    one(sources.layoutTests, new RegExp(`func ${name}\\(\\)`), `native expectation ${name}`);
  }
  one(sources.edgeTests, /func testTheTooltipLeavesByTheInwardFace\(\)/, 'native edge expectation');
  const l = reference.layout;
  const near = (actual, expected) => assert(Math.abs(actual - expected) < 0.001, `native expectation: ${actual} != ${expected}`);
  near(l.ringDiameter, 44);
  near(l.sideBodyDepth / l.ringDiameter, 186 / 117);
  near(l.cardWidth / l.ringDiameter, 600 / 117);
  assert(l.activityDiameter / 2 + l.activityStroke / 2 < l.ringDiameter / 2 - l.trackStroke);
  assert(l.activityDiameter / 2 - l.activityStroke / 2 > l.glyphSize / 2);
  assert(l.weeklyInsideRadius - l.weeklyRingStroke / 2 > l.glyphSize / 2);
  assert(l.weeklyInsideRadius + l.weeklyRingStroke / 2 < l.activityDiameter / 2 - l.activityStroke / 2);
  assert(l.weeklyOutsideRadius - l.weeklyRingStroke / 2 > l.ringDiameter / 2);
  assert(l.weeklyOutsideRadius + l.weeklyRingStroke / 2 < l.ringDiameter / 2 + l.sideRingMargin);
  assert(l.weeklyRingStroke < l.progressStroke);
  assert(l.localArcMinimumSweep * Math.PI * (l.ringDiameter - l.progressStroke) > 2 * l.progressStroke);
  assert(l.localArcMinimumSweep < 0.1);
  near(reference.motion.stagger.capSeconds, 0.18);
  assert.equal(reference.motion.reduceMotion, 'none');
  assert.deepEqual(Object.fromEntries(EDGES.map(edge => [edge, reference.anchors[edge].tooltipDirection])), { right: 'leading', left: 'trailing', top: 'down', bottom: 'up' });
  for (const edge of EDGES) {
    const a = reference.anchors[edge];
    assert.equal(a.outward.x * a.along.x + a.outward.y * a.along.y, 0);
    assert.equal(Math.hypot(a.outward.x, a.outward.y), 1);
    assert.equal(Math.hypot(a.along.x, a.along.y), 1);
  }
  for (const name of COUNTS) assert(Number.isSafeInteger(l[name]));
}

export function checkRuntimeLiteral(source, reference) {
  const start = '/* BEGIN GENERATED MAC REFERENCE */';
  const end = '/* END GENERATED MAC REFERENCE */';
  assert.equal(source.split(start).length, 2, 'runtime: expected one begin marker');
  assert.equal(source.split(end).length, 2, 'runtime: expected one end marker');
  const literal = source.slice(source.indexOf(start) + start.length, source.indexOf(end)).trim();
  assert(isDeepStrictEqual(JSON.parse(literal), reference), 'runtime generated reference differs from canonical JSON');
}

function selfTest(sources, reference) {
  assert.equal(numberExpression('2 * (3 + 4) / 7'), 2);
  for (const input of ['unknown', 'Design.px(12) + unknown', 'Math.max(1, 2)', '44 trailing', '1 / 0', '1 +', '0;process.exit()']) {
    assert.throws(() => numberExpression(input, {}, 44 / 117), undefined, input);
  }
  const mutate = (key, from, to) => {
    assert(sources[key].includes(from), `self-test fixture missing: ${from}`);
    return { ...sources, [key]: sources[key].replace(from, to) };
  };
  const changed = generateReference(mutate('layout', 'Design.px(600)', 'Design.px(600.01)'));
  assert.notEqual(changed.layout.cardWidth, reference.layout.cardWidth);
  assert.notEqual(serialize(changed), serialize(reference));
  assert.throws(() => generateReference(mutate('layout', 'Design.px(600)', 'Design.px(600) + unknown')), /unknown name/);
  assert.throws(() => generateReference(mutate('layout', 'Design.px(600)', 'Design.px(600)\n        + 12')), /unsupported continuation/);
  assert.throws(() => generateReference(mutate('layout', 'static let cardWidth', 'static let missingCardWidth')), /expected one match/);
  assert.throws(() => generateReference(mutate('layout', 'static let cardWidth', 'static let cardWidth = Design.px(600)\n    static let cardWidth')), /expected one match/);
  assert.throws(() => generateReference(mutate('design', 'pixels * scale', 'pixels * scale + 1')), /unsupported Design.px/);
  assert.throws(() => generateReference(mutate('motion', 'Double(index) * 0.045', 'Double(index) * newStep')), /unsupported stagger/);
  assert.throws(() => generateReference(mutate('providerRing', 'isStale ? (reduceTransparency ? 0.75 : 0.45)', 'isStale ? (reduceTransparency ? 0.75 : newOpacity)')), /expected one match/);
  assert.throws(() => generateReference(mutate('edge', 'case .top:    return .down', 'case .top:    return .up')), /deep-equal/);
  assert.throws(() => generateReference(mutate('layout', 'Design.px(72)', 'Design.px(110)')));
  const wrapped = value => `const reference = /* BEGIN GENERATED MAC REFERENCE */${JSON.stringify(value)}/* END GENERATED MAC REFERENCE */;`;
  checkRuntimeLiteral(wrapped(reference), reference);
  assert.throws(() => checkRuntimeLiteral(wrapped(changed), reference), /differs/);
  assert.throws(() => checkRuntimeLiteral('const reference = {};', reference), /begin marker/);
  console.log('PASS: parser fail-closed, source drift, native expectation and runtime drift checks');
}

async function main() {
  const args = new Set(process.argv.slice(2));
  for (const arg of args) assert(['--check', '--write', '--self-test', '--runtime'].includes(arg), `unknown option: ${arg}`);
  assert(!(args.has('--check') && args.has('--write')), '--check and --write are mutually exclusive');
  const entries = await Promise.all(Object.entries(SOURCE).map(async ([key, path]) => [key, await readFile(resolve(ROOT, path), 'utf8')]));
  const sources = Object.fromEntries(entries);
  const reference = generateReference(sources);
  if (args.has('--self-test')) selfTest(sources, reference);
  const outputPath = resolve(ROOT, OUTPUT);
  if (args.has('--write')) {
    // Do not follow a changed destination symlink outside this repository.
    const actualRoot = await realpath(ROOT);
    const actualParent = await realpath(dirname(outputPath));
    assert(!relative(actualRoot, actualParent).startsWith('..'), 'output directory escapes repository');
    try {
      assert.equal(await realpath(outputPath), resolve(actualParent, 'mac-notch-reference.json'), 'output is a symlink');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await writeFile(outputPath, serialize(reference));
    console.log(`WROTE ${OUTPUT}`);
  } else {
    assert((await readFile(outputPath, 'utf8')) === serialize(reference), 'Mac reference drift: review the native change, then run node scripts/check-mac-notch-reference.mjs --write');
    console.log(`PASS: ${OUTPUT} matches ${Object.keys(SOURCE).length} source fingerprints`);
  }
  if (args.has('--runtime')) {
    checkRuntimeLiteral(await readFile(resolve(ROOT, RUNTIME), 'utf8'), reference);
    console.log(`PASS: ${RUNTIME} embeds the current reference`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
