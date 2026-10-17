(() => { try { return JSON.stringify((() => {
            const value = rt.extendInstance({}, [['ILookup', 0],
                ['ILookup/-lookup', 2, () => 20], ['ILookup/-lookup', 3, () => 30]], true);
            const previous = Object.getOwnPropertyDescriptor(Array.prototype, '3');
            try {
                Object.defineProperty(Array.prototype, '3', {
                    value: 'wrong-slot', configurable: true, writable: true
                });
                return rt.core.get(value, 'key', 'fallback');
            } finally {
                if (previous) Object.defineProperty(Array.prototype, '3', previous);
                else delete Array.prototype[3];
            }
        })()); } catch (error) { return JSON.stringify({error: error.message}); } })()
