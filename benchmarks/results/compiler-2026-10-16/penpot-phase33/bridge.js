    function macroArguments(form, nodes, depth) {
      const input = macroInputs.get(form);
      if (!input || input.root[0] !== "list") throw new Error("Missing compiler macro input");
      const args = [], snapshots = [];
      try {
        for (const descriptor of input.root[3].slice(1)) {
          // Restore the same per-argument metadata/provenance order as a new
          // import, without reconstructing the objects already in &form.
          const pending = [descriptor, false];
          while (pending.length) {
            const ready = pending.pop(), d = pending.pop();
            if (ready) { decodeNode(d, input.values, true); continue; }
            pending.push(d, true);
            if (d[2] != null) pending.push(d[2], false);
            switch (d[0]) {
              case "list": case "vector": case "map": case "set":
                for (let i = d[3].length - 1; i >= 0; --i) pending.push(d[3][i], false);
                break;
              case "regex": case "instant": case "uuid": case "queue": case "js-array": case "js-object":
                pending.push(d[3], false); break;
            }
          }
          const value = input.values.get(descriptor);
          args.push(value); snapshots.push(encode(value, nodes, depth));
        }
        return [args, snapshots];
      } finally { macroInputs.delete(form); }
    }
