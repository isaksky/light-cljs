#!/usr/bin/env python3
"""Direct Unix terminal, cancellation and limit checks for the built CLI."""
import os, pty, signal, subprocess, time
from pathlib import Path
binary = os.environ.get('LIGHT_CLJS_BINARY', str(Path(__file__).resolve().parents[2] / 'target/debug/light-cljs'))
env=dict(os.environ, PATH='')
def run(args, expected):
 p=subprocess.run([binary,*args],text=True,capture_output=True,env=env,timeout=10)
 assert p.returncode==0 and p.stdout==expected and p.stderr=='',(args,p.returncode,p.stdout,p.stderr)
 print('PASS',args)
run(['--eval','--text','(/ 1 2)'],'0.5\n')
run(['--eval','--text','(defmacro twice [x] (* x 2)) [(twice 21) (js/Math.sqrt 81)]'],'[42 9]\n')
p=subprocess.run([binary,'--eval','--text','(require \'missing)'],text=True,capture_output=True,env=env,timeout=10)
assert p.returncode!=0 and 'no source provider for missing' in p.stderr and not p.stdout
print('PASS missing explicit source',p.stderr.strip())
for flag,value,source,expected in [('--stack-limit','65536','((fn f [] (f)))','stack'),('--memory-limit','4194304','(try (js* "(()=>{const a=[]; while(true) a.push(new Array(4096).fill(1));})()") (catch :default e 42))','memory')]:
 p=subprocess.run([binary,'--eval',flag,value,'--text',source],text=True,capture_output=True,env=env,timeout=15)
 assert p.returncode!=0 and expected in p.stderr.lower(),(flag,p.returncode,p.stderr)
 print('PASS limit',flag,p.stderr[:120].strip())
p=subprocess.Popen([binary,'--eval','--text','(println :ready) (loop [] (recur))'],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
assert p.stdout.readline()==':ready\n'
p.send_signal(signal.SIGINT)
p.communicate(timeout=5)
assert p.returncode!=0
print('PASS cancellation exits',p.returncode)
run(['--eval','--text','42'],'42\n')
master,slave=pty.openpty()
p=subprocess.Popen([binary,'--repl'],stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
os.close(slave)
os.write(master,'(ns café)\n(+ 20\n 22)\n'.encode())
time.sleep(.2)
os.write(master,b'\x04')
stdout,stderr=p.communicate(timeout=10)
os.close(master)
assert p.returncode==0 and stdout==b'nil\n42\n',(p.returncode,stdout,stderr)
assert 'cljs.user=> ' in stderr.decode() and 'café=> ' in stderr.decode() and '...=> ' in stderr.decode(),stderr
print('PASS terminal prompts/continuation/EOF',repr(stderr.decode()))
