    fn macro_arguments(
        &self,
        form: ValueId,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        let form = self.check(form)?;
        let indices: Vec<u64> = self.enter(|ctx| {
            let import: Function = self.services.get("macroArguments")?;
            let result: Array = import.call((form, limits.nodes, limits.depth))?;
            let indices: Vec<u64> = result.get(0)?;
            let snapshot: rquickjs::Result<()> = (|| {
                let snapshots: Array = result.get(1)?;
                for item in snapshots.iter::<Array>() {
                    self.bridge.borrow_mut().decode(item?, self.control, limits, true)?;
                }
                Ok(())
            })();
            snapshot.map_err(|error| {
                let exception = error.is_exception().then(|| ctx.catch());
                for index in &indices {
                    let release: rquickjs::Result<Function> = self.services.get("release");
                    if release.and_then(|f| f.call::<_, ()>((*index,))).is_err() { let _ = ctx.catch(); }
                }
                if let Some(exception) = exception { let _ = ctx.throw(exception); }
                error
            })?;
            Ok(indices)
        })?;
        Ok(indices.into_iter().map(|index| ValueId {session: self.generation, index}).collect())
    }
