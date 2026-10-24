"""Generate the shared traffic classes and encoded carrier cost contract."""


def render(schema):
    """Render both languages from the same exhaustive control classification."""
    budget = schema['rate_budget']
    if budget['size'] != 'encoded_carrier_bytes':
        raise ValueError('unsupported rate cost unit')
    classes = budget['classes']
    control = next(e for e in schema['enums'] if e['name'] == 'Control')
    rust = ['#[derive(Clone, Copy, Debug, PartialEq, Eq)]', 'pub enum RateClass {',
            ','.join(classes), '}', 'impl RateClass {',
            '/// Counts encoded carrier bytes and messages, including JSON escaping and fragment headers.',
            'pub fn cost(self, sizes: impl IntoIterator<Item = usize>) -> anyhow::Result<RateCost> {',
            'let mut cost = RateCost::default(); for size in sizes {',
            'anyhow::ensure!(size <= protocol::MAX_EXPERIENCE_ENVELOPE_BYTES, BurstExceeded);',
            'cost.messages += 1; cost.bytes += size as u64;',
            'if cost.messages > crate::policy::MAX_MESSAGES_PER_SECOND || cost.bytes > crate::policy::MAX_BURST_BYTES { return Err(BurstExceeded.into()); }',
            '}', 'match self {']
    go = ['type RateClass uint8', 'const (']
    go += [f'Rate{name}' + (' RateClass = iota' if i == 0 else '') for i, name in enumerate(classes)]
    go += [')', '// Cost counts encoded carrier bytes and messages, including escaping and fragment headers.',
           'func (class RateClass) Cost(sizes []int) (RateCost, error) {',
           'var cost RateCost; for _, size := range sizes {',
           'if size < 0 { return RateCost{}, errors.New("negative carrier size") }; if size > MaxEnvelopeBytes { return RateCost{}, &BurstExceeded{} }',
           'cost.Messages++; cost.Bytes += uint64(size)',
           'if cost.Messages > MaxMessagesPerSecond || cost.Bytes > MaxBurstBytes { return RateCost{}, &BurstExceeded{} }',
           '}', 'switch class {']
    for name, charged in classes.items():
        rust += [f'Self::{name} => Ok(' + ('cost' if charged else 'RateCost::default()') + '),']
        go += [f'case Rate{name}:', 'return ' + ('cost' if charged else 'RateCost{}') + ', nil']
    rust += ['}', '}', '}', '#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]',
             'pub struct RateCost { pub messages: u64, pub bytes: u64 }',
             '/// A batch that cannot fit even when the rate bucket is full.',
             '#[derive(Debug)]', 'pub struct BurstExceeded;',
             'impl std::fmt::Display for BurstExceeded {',
             '/// Explains why retrying the unchanged batch cannot succeed.',
             'fn fmt(&self, f: &mut std::fmt::Formatter<\'_>) -> std::fmt::Result { f.write_str("encoded message too large for maximum rate burst") }',
             '}', 'impl std::error::Error for BurstExceeded {}',
             '#[cfg(feature = "host")]', 'impl crate::session::Control {',
             '/// Classifies a control in its wire direction; only an outstanding notice acknowledgment is exempt.',
             'pub fn rate_class(&self, direction: crate::wire::Direction, matching_notice: bool) -> RateClass {',
             'use crate::wire::Direction; match self {']
    go += ['default: return RateCost{}, errors.New("unknown rate class")', '}', '}',
           'type RateCost struct { Messages, Bytes uint64 }',
           '// BurstExceeded means the encoded batch cannot fit even in a full bucket.',
           'type BurstExceeded struct {}',
           '// Error explains why retrying the unchanged batch cannot succeed.',
           'func (*BurstExceeded) Error() string { return "encoded message too large for maximum rate burst" }',
           '// RateClass classifies a control; only an outstanding notice acknowledgment is exempt.',
           'func (c Control) RateClass(direction Direction, matchingNotice bool) RateClass {', 'switch {']
    for v in control['variants']:
        rate = v['rate']
        if not set(rate.values()) <= classes.keys():
            raise ValueError('unknown traffic class')
        pattern = f'Self::{v["rust"]}' + ('(..)' if 'type' in v else ' { .. }' if v.get('fields') else '')
        rust += [pattern + ' => {']
        go += ['case ' + (f'c.{v["go"]}' if v['json'] == 'disabled' else f'c.{v["go"]} != nil') + ':']
        if 'matching_notice' in rate:
            rust += [f'if direction == Direction::ToServer && matching_notice {{ return RateClass::{rate["matching_notice"]}; }}']
            go += [f'if direction == ToServer && matchingNotice {{ return Rate{rate["matching_notice"]} }}']
        if rate['to_client'] == rate['to_server']:
            rust += [f'RateClass::{rate["to_client"]}']
            go += [f'return Rate{rate["to_client"]}']
        else:
            rust += [f'match direction {{ Direction::ToClient => RateClass::{rate["to_client"]}, Direction::ToServer => RateClass::{rate["to_server"]} }}']
            go += [f'if direction == ToClient {{ return Rate{rate["to_client"]} }}; return Rate{rate["to_server"]}']
        rust += ['},']
    rust += ['}', '}', '}', '#[cfg(feature = "host")]', 'impl crate::negotiation::Limits {',
             '/// Lowers the payload ceiling so UTF-8 fragmentation fits the message burst.',
             'pub fn fit_rate(mut self) -> Self {',
             'self.max_message_bytes = self.max_message_bytes.min(self.max_fragment_bytes.saturating_sub(3).saturating_mul(crate::policy::MAX_MESSAGES_PER_SECOND as u32));',
             'self', '}', '}']
    go += ['default: return RateData', '}', '}',
           '// fitRate lowers the payload ceiling so UTF-8 fragmentation fits the message burst.',
           'func (l Limits) fitRate() Limits {',
           'if l.MaxFragmentBytes < 4 { l.MaxMessageBytes = 0; return l }',
           'l.MaxMessageBytes = uint32(min(uint64(l.MaxMessageBytes), uint64(l.MaxFragmentBytes-3)*MaxMessagesPerSecond))',
           'return l', '}']
    return '\n'.join(rust) + '\n', '\n'.join(go) + '\n'
