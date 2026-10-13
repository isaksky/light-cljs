;; Compiler-owned clojure.string implementation, based on the public contracts
;; of ClojureScript clojure.string (differences: docs/compiler.md).
;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns clojure.string (:refer-clojure :exclude [replace reverse]))

(defn reverse
  "Returns s with its characters reversed, preserving UTF-16 surrogate pairs."
  [s]
  (js* "~{s}.replace(/([\\uD800-\\uDBFF])([\\uDC00-\\uDFFF])/g, '$2$1').split('').reverse().join('')"))

(defn- replace* [s match replacement first?]
  (js* "((s, match, replacement, first, vec) => {
    if (typeof s !== 'string') throw new TypeError('replace requires a string');
    if (typeof match === 'string') {
      if (typeof replacement !== 'string') throw new TypeError('literal replacement requires a string');
      let result = '', cursor = 0;
      while (cursor <= s.length) {
        const index = s.indexOf(match, cursor);
        if (index < 0) return result + s.substring(cursor);
        result += s.substring(cursor, index) + replacement;
        cursor = index + match.length;
        if (first) return result + s.substring(cursor);
        if (match.length === 0) {
          if (cursor === s.length) return result;
          result += s.charAt(cursor++);
        }
      }
      return result;
    }
    if (!(match instanceof RegExp)) throw new TypeError('Invalid match arg: ' + match);
    const regex = new RegExp(match.source, match.flags.replace(/[gy]/g, '') + (first ? '' : 'g'));
    if (typeof replacement === 'string') return s.replace(regex, replacement);
    return s.replace(regex, (...args) => {
      const end = args.length - (typeof args[args.length - 1] === 'object' ? 3 : 2);
      const matches = args.slice(0, end);
      return replacement.call(null, matches.length === 1 ? matches[0] : vec(matches));
    });
  })(~{s}, ~{match}, ~{replacement}, ~{first?}, ~{vec})"))

(defn replace
  "Replaces every match. String matches use literal replacements; regex matches
  accept a replacement string with $ groups or a function of the match."
  [s match replacement]
  (replace* s match replacement false))

(defn replace-first
  "Replaces the first match, with the same replacement rules as replace."
  [s match replacement]
  (replace* s match replacement true))

(defn join
  "Joins the str of each element of coll, with an optional separator."
  ([coll] (join "" coll))
  ([separator coll]
   (loop [remaining (seq coll) result ""]
     (if remaining
       (let [result (str result (first remaining))
             remaining (next remaining)]
         ;; StringBuffer appends separators directly, after realizing next.
         ;; In particular, nil separates elements with "null", not "".
         (recur remaining
           (if remaining (js* "~{} + String(~{})" result separator) result)))
       result))))

(defn upper-case "Converts string to all upper-case." [s] (.toUpperCase s))
(defn lower-case "Converts string to all lower-case." [s] (.toLowerCase s))
(defn capitalize
  "Upper-cases the first UTF-16 character and lower-cases the rest."
  [s]
  (js* "~{s}.charAt(0).toUpperCase() + ~{s}.substring(1).toLowerCase()"))

(defn split
  "Splits s on a regex into a vector. A positive limit retains the unsplit
  remainder in the last part; zero discards trailing empty parts."
  ([s re] (split s re 0))
  ([s re limit]
   (vec
     (js* "((s, re, limit) => {
       let parts;
       if (re.source === '(?:)' && re.flags === '') {
         if (limit <= 0 || limit >= s.length + 2) parts = [''].concat(s.split(''), ['']);
         else if (limit === 1) parts = [s];
         else parts = [''].concat(s.substring(0, limit - 2).split(''), [s.substring(limit - 2)]);
       } else if (limit < 1) {
         parts = s.split(re);
       } else {
         parts = [];
         const regex = new RegExp(re.source, re.flags.replace(/[gy]/g, ''));
         let remaining = s;
         while (parts.length < limit - 1) {
           const match = regex.exec(remaining);
           if (match === null) break;
           parts.push(remaining.substring(0, match.index));
           remaining = remaining.substring(match.index + match[0].length);
         }
         parts.push(remaining);
       }
       if (limit === 0 && parts.length > 1) {
         while (parts.length && parts[parts.length - 1] === '') parts.pop();
       }
       return parts;
     })(~{}, ~{}, ~{})" (str s) re limit))))

(defn split-lines "Splits on LF or CRLF, discarding trailing empty lines." [s]
  (split s #"\n|\r\n"))
(defn trim "Removes whitespace from both ends of s." [s]
  (js* "~{s}.replace(/^[\\s\\xa0]+|[\\s\\xa0]+$/g, '')"))
(defn triml "Removes whitespace from the left side of s." [s]
  (js* "~{s}.replace(/^[\\s\\xa0]+/, '')"))
(defn trimr "Removes whitespace from the right side of s." [s]
  (js* "~{s}.replace(/[\\s\\xa0]+$/, '')"))
(defn trim-newline "Removes all trailing LF and CR characters." [s]
  (js* "~{s}.replace(/[\\n\\r]+$/, '')"))
(defn blank? "True if s is nil, empty, or contains only whitespace." [s]
  (js* "~{s} == null || /^[\\s\\xa0]*$/.test(~{s})"))

(defn escape
  "Replaces each UTF-16 character with (str (cmap ch)) when that value is non-nil."
  [s cmap]
  (js* "((s, cmap, str) => {
    let result = '';
    for (let index = 0; index < s.length; index++) {
      const ch = s.charAt(index), replacement = cmap.call(null, ch);
      result += replacement == null ? ch : str(replacement);
    }
    return result;
  })(~{s}, ~{cmap}, ~{str})"))

(defn index-of
  "Returns the UTF-16 index of value, or nil, optionally starting from from-index."
  ([s value] (index-of s value 0))
  ([s value from-index]
   (let [result (.indexOf s value from-index)] (if (neg? result) nil result))))
(defn last-index-of
  "Returns the last UTF-16 index of value, or nil, optionally searching backwards."
  ([s value]
   (let [result (.lastIndexOf s value)] (if (neg? result) nil result)))
  ([s value from-index]
   (let [result (.lastIndexOf s value from-index)] (if (neg? result) nil result))))
(defn starts-with? "True if s starts with substr." [s substr]
  (js* "~{s}.lastIndexOf(~{substr}, 0) === 0"))
(defn ends-with? "True if s ends with substr." [s substr]
  (js* "((s, part) => { const index = s.length - part.length; return index >= 0 && s.indexOf(part, index) === index; })(~{s}, ~{substr})"))
(defn includes? "True if s includes substr." [s substr]
  (js* "~{s}.indexOf(~{substr}) !== -1"))
