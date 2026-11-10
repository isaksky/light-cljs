function regexpFormValues() {
  'use strict';
  // Shareable regex values on Goose (light-cljs-935; the Pants engine's
  // regexp/shared.rs, collections/form.rs new_regex and the shared-profile
  // tests of tests/bbp_macros/shared_profile.rs): `__rt.formRegex(source,
  // flags)` is a RegExp in every JavaScript respect, exec, test, replace,
  // the getters and the brand checks, whose `lastIndex` stays 0: a write of
  // 0 is accepted and changes nothing, anything else is a TypeError, as is
  // a definition that would change it or make it read-only and freezing
  // it; nothing can be added. RegExp(r) is r, new RegExp(r) an ordinary
  // copy. Global, sticky, invalid and non-string patterns are refused.
  // Goose only: Node has no `__rt`.
  const r = __rt.formRegex('(a)(?<n>b)?', 'dm');
  const m = r.exec('xab');
  const exec = m[0] === 'ab' && m[1] === 'a' && m.groups.n === 'b' && m.index === 1 && m.indices[0][0] === 1 && m.indices[0][1] === 3;
  const test = r.test('zzab') && !r.test('zz');
  const replace = 'xaab'.replace(r, '[$1]') === 'x[a]ab' && 'ab ab'.replace(r, (s) => '<' + s + '>') === '<ab> ab';
  const getters = r.source === '(a)(?<n>b)?' && r.flags === 'dm' && r.hasIndices && r.multiline && !r.global && !r.sticky && String(r) === '/(a)(?<n>b)?/dm';
  r.lastIndex = 0;
  r.lastIndex = -0;
  let one = false;
  try { r.lastIndex = 1; } catch (e) { one = e instanceof TypeError && e.message === "unsupported operation: a shared regex value's lastIndex stays 0 (global and sticky regexes are not shared)"; }
  let text = false;
  try { r.lastIndex = '0'; } catch (e) { text = e instanceof TypeError; }
  const own = Object.getOwnPropertyDescriptor(r, 'lastIndex');
  const last = Object.is(r.lastIndex, 0) && Object.getOwnPropertyNames(r).join() === 'lastIndex' && own.writable && !own.enumerable && !own.configurable;
  let define = false;
  try { Object.defineProperty(r, 'lastIndex', { value: 2 }); } catch (e) { define = e instanceof TypeError; }
  let readOnly = false;
  try { Object.defineProperty(r, 'lastIndex', { writable: false }); } catch (e) { readOnly = e instanceof TypeError && e.message === "unsupported operation: a shared regex value's lastIndex stays writable"; }
  Object.defineProperty(r, 'lastIndex', { value: 0 });
  const refusedDefinition = !Reflect.defineProperty(r, 'extra', { value: 1 }) && !Reflect.defineProperty(r, 'lastIndex', { enumerable: true });
  let added = false;
  try { r.extra = 1; } catch (e) { added = e instanceof TypeError; }
  const sealed = !Object.isExtensible(r) && Object.isSealed(r) && !Object.isFrozen(r) && !('extra' in r);
  let frozen = false;
  try { Object.freeze(r); } catch (e) { frozen = e instanceof TypeError; }
  const kept = Object.seal(r) === r && Object.preventExtensions(r) === r && !Reflect.deleteProperty(r, 'lastIndex') && delete r.missing;
  const brand = r instanceof RegExp && Object.getPrototypeOf(r) === RegExp.prototype && Object.prototype.toString.call(r) === '[object RegExp]';
  const copy = new RegExp(r);
  copy.lastIndex = 3;
  const identity = RegExp(r) === r && copy !== r && copy.source === r.source && copy.flags === 'dm' && copy.lastIndex === 3 && Object.isExtensible(copy) && r.lastIndex === 0;
  const g = new RegExp(r, 'g');
  const stateful = g.exec('ab ab')[0] === 'ab' && g.lastIndex === 2;
  const refused = (source, flags) => { try { __rt.formRegex(source, flags); return false; } catch (e) { return e instanceof TypeError; } };
  const refusals = refused('a', 'g') && refused('a', 'y') && refused('a', 'gu') && refused('(', '') && refused('a', 'x') && refused(1, '') && refused('a', undefined);
  // As a form it is the Pants engine's regex compiler literal; a copy is none.
  const form = __rt.formKind(r) === 'compiler-literal' && __rt.formName(r) === 'regex' && __rt.formNamespace(r) === null && __rt.formKind(copy) === null;
  const plain = __rt.formRegex('', '');
  const empty = plain.source === '(?:)' && plain.flags === '' && plain.test('x') && RegExp(plain) === plain;
  return exec && test && replace && getters && one && text && last && define && readOnly && refusedDefinition && added && sealed && frozen && kept && brand && identity && stateful && refusals && form && empty;
}
