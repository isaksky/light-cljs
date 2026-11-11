function unsupportedArrayMethod() { var a = [1, 2]; a.tag = 1; a.concat(); return a.length; }
