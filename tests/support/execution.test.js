// The outcome bridge must capture realization effects and restore streams even
// when serialization fails. These checks use the same runner as both compilers.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const runner = fs.readFileSync(__dirname + '/execution.js', 'utf8');
function context(inspect, program) {
  const output = [];
  const write = text => { output.push(String(text)); return true; };
  const process = { stdout: { write }, stderr: { write: () => true } };
  const sandbox = { Buffer, process, lightCases: [program(process)],
    lightInspectValue: value => inspect(value, process), lightCaseMetadata: [['realization', 'retained lazy value']] };
  return { sandbox, output, write };
}
const result = context((value, process) => {
  if (!Object.hasOwn(value, 'label')) return null;
  process.stdout.write(value.label);
  return { kind: 'seq', items: [value.number] };
}, process => observe => {
  process.stdout.write('body;');
  observe({ label: 'trace;', number: 2 });
  return { label: 'value;', number: 1 };
});
vm.runInNewContext(runner, result.sandbox);
assert.equal(result.sandbox.process.stdout.write, result.write);
assert.equal(result.output.length, 1);
assert.deepEqual(JSON.parse(result.output[0]), { id: 'realization', outcome: { status: 'returned', value: { $seq: [1] } }, trace: [{ $seq: [2] }], stdout: 'body;value;trace;' });
const failure = context(() => { throw new Error('realization failed'); }, () => () => ({}));
assert.throws(() => vm.runInNewContext(runner, failure.sandbox), /realization failed/);
assert.equal(failure.sandbox.process.stdout.write, failure.write);
assert.deepEqual(failure.output, []);
console.log('Outcome realization capture and stream recovery passed');
