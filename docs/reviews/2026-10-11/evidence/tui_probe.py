import os,pty,subprocess,threading,time,json,sqlite3,tempfile,select,fcntl,termios,struct
from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
from pathlib import Path
root=Path('/tmp/alice-audit-20261011')
db=sqlite3.connect(root/'fixtures/mail.sqlite3')
account=json.loads(db.execute("select data from accounts where id='demo-work'").fetchone()[0])
message=json.loads(db.execute("select data from messages where id='demo-1'").fetchone()[0]);db.close()
state={'accounts':[account],'messages':[message],'delay':0,'saved':[],'patch_event':threading.Event(),'loaded':threading.Event()}
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def out(self,data):
  b=json.dumps(data).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
 def do_GET(self):
  if self.path.startswith('/v1/health'): self.out({'status':'ok','version':'0.1.0'})
  elif self.path.startswith('/v1/accounts'):self.out(state['accounts'])
  else:self.out({'messages':state['messages'],'total':len(state['messages'])});state['loaded'].set()
 def do_PATCH(self):
  state['patch_event'].set();time.sleep(state['delay']);self.out(message)
 def do_POST(self):
  draft=json.loads(self.rfile.read(int(self.headers['Content-Length'])));state['saved'].append(draft)
  out=dict(message,id=draft['id'],to=draft['to'],body=draft['body'],subject=draft['subject'],account_id=draft['account_id'],folder='drafts');self.out(out)
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
binary=str(Path('target/debug/alice').resolve())
def start():
 fixture=Path(tempfile.mkdtemp(prefix='alice-tui-audit-'));(fixture/'daemon.token').write_text('fixture-token')
 master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',36,120,0,0))
 proc=subprocess.Popen(['rtk','proxy',binary],stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,ALICE_DATA_DIR=str(fixture),ALICE_PORT=str(server.server_port)))
 os.close(slave)
 def drain():
  while proc.poll() is None:
   try:
    if select.select([master],[],[],.1)[0]:os.read(master,65536)
   except OSError:break
 threading.Thread(target=drain,daemon=True).start()
 state['loaded'].clear();state['loaded'].wait(3);time.sleep(.2)
 return proc,master,fixture
results={}
proc,master,fixture=start()
try:
 os.write(master,b'c');time.sleep(.2);os.write(master,b'person@example.invalid\tFixture\tHello\x1b[DX\x13')
 for _ in range(100):
  if state['saved']:break
  time.sleep(.02)
 results['editor_left_actual_body']=state['saved'][-1]['body'];results['editor_left_expected_body']='HellXo'
 time.sleep(.3);os.write(master,b'q');proc.wait(timeout=3)
finally:
 if proc.poll() is None:proc.kill();proc.wait()
 os.close(master)
 import shutil;shutil.rmtree(fixture)
state['saved']=[];state['delay']=4;state['patch_event'].clear()
proc,master,fixture=start()
try:
 os.write(master,b'\r');state['patch_event'].wait(3);start_time=time.monotonic();os.write(master,b'q');time.sleep(1)
 results['q_ignored_while_patch_pending']=proc.poll() is None
 proc.wait(timeout=6);results['exit_delay_seconds']=round(time.monotonic()-start_time,2)
finally:
 if proc.poll() is None:proc.kill();proc.wait()
 os.close(master);shutil.rmtree(fixture)
state['saved']=[];state['delay']=0
proc,master,fixture=start()
try:
 new=dict(account,id='new-real',email='real@example.invalid',is_demo=False);state['accounts']=[new];state['messages']=[];state['loaded'].clear();state['loaded'].wait(3);time.sleep(.2)
 os.write(master,b'c');time.sleep(.2);os.write(master,b'person@example.invalid\tFixture\tNew account test\x13')
 for _ in range(100):
  if state['saved']:break
  time.sleep(.02)
 results['compose_after_external_connect_actual_account']=state['saved'][-1]['account_id'];results['compose_after_external_connect_expected_account']='new-real'
 time.sleep(.3);os.write(master,b'q');proc.wait(timeout=3)
finally:
 if proc.poll() is None:proc.kill();proc.wait()
 os.close(master);shutil.rmtree(fixture)
server.shutdown();server.server_close();(root/'tui-evidence.json').write_text(json.dumps(results,indent=2));print(json.dumps(results,indent=2))
