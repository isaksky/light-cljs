//! Private, owner-scoped slots. Reclaimed slots change generation before reuse.

use super::HandleError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct Key {
    pub(super) owner: u64,
    slot: u32,
    generation: u32,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
    marked: bool,
}

pub(super) struct Arena<T> {
    owner: u64,
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
    revision: u64,
}

impl<T> Arena<T> {
    pub(super) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            revision: 0,
        }
    }

    fn changed(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("arena revisions exhausted");
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn insert(&mut self, value: T) -> Key {
        self.changed();
        let slot = if let Some(index) = self.free.pop() {
            self.slots[index as usize].value = Some(value);
            index
        } else {
            let index = u32::try_from(self.slots.len()).expect("interpreter arena exhausted");
            self.slots.push(Slot {
                generation: 0,
                value: Some(value),
                marked: false,
            });
            index
        };
        self.len += 1;
        Key {
            owner: self.owner,
            slot,
            generation: self.slots[slot as usize].generation,
        }
    }

    fn slot(&self, key: Key) -> Result<&Slot<T>, HandleError> {
        if key.owner != self.owner {
            return Err(HandleError::WrongOwner);
        }
        self.slots
            .get(key.slot as usize)
            .filter(|slot| slot.generation == key.generation && slot.value.is_some())
            .ok_or(HandleError::StaleId)
    }

    pub(super) fn get(&self, key: Key) -> Result<&T, HandleError> {
        Ok(self.slot(key)?.value.as_ref().expect("occupied slot"))
    }

    /// Returns the record only on its first visit in this collection.
    pub(super) fn mark(&mut self, key: Key) -> Option<&T> {
        self.slot(key)
            .expect("runtime edge must address a live record");
        let slot = &mut self.slots[key.slot as usize];
        if slot.marked {
            return None;
        }
        slot.marked = true;
        slot.value.as_ref()
    }

    pub(super) fn sweep(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.value.is_some() && !slot.marked {
                slot.value = None;
                self.len -= 1;
                // Retire an exhausted slot permanently, never wrap an identity.
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    self.free.push(index as u32);
                }
            }
            slot.marked = false;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn slots(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn get_mut(&mut self, key: Key) -> &mut T {
        self.slot(key).unwrap();
        self.changed();
        self.slots[key.slot as usize].value.as_mut().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exhausted_generation_retires_the_slot() {
        let mut arena = Arena::new(1);
        let old = arena.insert("old");
        arena.slots[old.slot as usize].generation = u32::MAX;
        let last = Key {
            generation: u32::MAX,
            ..old
        };
        arena.sweep();
        let new = arena.insert("new");
        assert_ne!(last.slot, new.slot);
        assert_eq!(arena.get(last), Err(HandleError::StaleId));
        assert_eq!(arena.get(new), Ok(&"new"));
    }
}
