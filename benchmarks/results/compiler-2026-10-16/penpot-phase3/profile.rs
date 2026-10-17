use std::{cell::RefCell,collections::BTreeMap,time::Instant};
#[derive(Default)] struct Stats { last:Option<Instant>, stack:Vec<&'static str>, times:BTreeMap<&'static str,(u128,usize)>, counts:BTreeMap<&'static str,u64> }
impl Stats { fn tick(&mut self){let now=Instant::now(); if let (Some(last),Some(name))=(self.last,self.stack.last()){self.times.entry(name).or_default().0+=now.duration_since(last).as_nanos();} self.last=Some(now);} }
thread_local! { static STATS:RefCell<Stats>=RefCell::new(Stats::default()); }
pub struct Guard;
pub fn scope(name:&'static str)->Guard{STATS.with(|s|{let mut s=s.borrow_mut();s.tick();s.stack.push(name);s.times.entry(name).or_default().1+=1;});Guard}
impl Drop for Guard{fn drop(&mut self){STATS.with(|s|{let mut s=s.borrow_mut();s.tick();s.stack.pop();});}}
pub fn count(name:&'static str,n:u64){STATS.with(|s|*s.borrow_mut().counts.entry(name).or_default()+=n);}
pub fn flag(name:&str)->bool{std::env::var_os(name).is_some()}
pub fn report(){STATS.with(|s|{let mut s=s.borrow_mut();s.tick();for(k,(ns,n))in &s.times{eprintln!("PROFILE {k} seconds={:.6} calls={n}",*ns as f64/1e9);}for(k,v)in &s.counts{eprintln!("COUNT {k} {v}");}});}
