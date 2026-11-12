function stringHtml() {
  // light-cljs-904: CreateHTML (ECMAScript B.2.2.2.1) converts a missing,
  // undefined or null attribute with ToString. QuickJS rejects nullish
  // attributes and the Pants engine lacks the methods, so only Goose runs it.
  let log = '';
  const value = {toString() { log += 'a'; return '"q"'; }};
  const receiver = {toString() { log += 'r'; return 'y'; }};
  return 'x'.anchor() === '<a name="undefined">x</a>'
    && 'x'.anchor(undefined) === '<a name="undefined">x</a>'
    && 'x'.link(null) === '<a href="null">x</a>'
    && 'x'.fontcolor() === '<font color="undefined">x</font>'
    && 'x'.fontsize(null) === '<font size="null">x</font>'
    && String.prototype.link.call(receiver, value) === '<a href="&quot;q&quot;">y</a>'
    && log === 'ra';
}
