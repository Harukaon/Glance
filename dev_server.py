import mimetypes
mimetypes.add_type("text/javascript", ".mjs")

from http.server import HTTPServer, SimpleHTTPRequestHandler
import os, sys, datetime

LOG_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "dev_server.log")

def log(msg):
    line = f"[{datetime.datetime.now().isoformat()}] {msg}"
    print(line, flush=True)
    with open(LOG_FILE, "a", encoding="utf-8") as f:
        f.write(line + "\n")

log("=== dev_server.py STARTING ===")
log(f"sys.argv: {sys.argv}")
log(f"__file__: {__file__}")
log(f"os.getcwd(): {os.getcwd()}")
log(f"script dir: {os.path.dirname(os.path.abspath(__file__))}")

ui_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ui")
log(f"resolved ui_dir: {ui_dir}")
log(f"ui_dir exists: {os.path.isdir(ui_dir)}")
if os.path.isdir(ui_dir):
    log(f"ui_dir contents: {os.listdir(ui_dir)}")

os.chdir(ui_dir)
log(f"after chdir, os.getcwd(): {os.getcwd()}")

class LoggingHandler(SimpleHTTPRequestHandler):
    def do_GET(self):
        log(f"REQUEST: GET {self.path}")
        log(f"  headers: {dict(self.headers)}")
        super().do_GET()

    def do_HEAD(self):
        log(f"REQUEST: HEAD {self.path}")
        super().do_HEAD()

    def log_message(self, format, *args):
        log(f"  {self.address_string()} - {format % args}")

    def send_response(self, code, message=None):
        log(f"  RESPONSE: {code} {message}")
        super().send_response(code, message)

    def send_header(self, keyword, value):
        log(f"  HEADER: {keyword}: {value}")
        super().send_header(keyword, value)

    def guess_type(self, path):
        guessed = super().guess_type(path)
        log(f"  guess_type({path}) => {guessed}")
        return guessed

server = HTTPServer(("127.0.0.1", 1420), LoggingHandler)
log("=== SERVER LISTENING on http://127.0.0.1:1420 ===")
server.serve_forever()
