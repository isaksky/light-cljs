(() => {
  "use strict";
  const truth = (value) => value != null && value !== false;
  {
    const l1 = (function(l0) {
      return (function() {
        return (function() {
          return l0;
        });
      });
    });
    const l2 = (l1).call(null, 8);
    return ((l2).call(null)).call(null);
  }
})()
