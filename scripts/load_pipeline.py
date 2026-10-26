"""Bounded, loopback-only load scenario. No existing database or deployment is touched."""
import argparse
from collections import Counter
from contextlib import ExitStack
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import queue
import secrets
import subprocess
import tempfile
import threading
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--messages', type=int, default=200)
    parser.add_argument('--workers', type=int, default=8)
    parser.add_argument('--payload-bytes', type=int, default=1024)
    parser.add_argument('--capacity', type=int, default=50)
    parser.add_argument('--scenario', choices=['healthy', 'slow', 'failure'], default='slow')
    parser.add_argument('--drain-seconds', type=float, default=15)
    parser.add_argument('--binary', type=Path, default=ROOT/'target/release/synquay-server')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not (1 <= args.messages <= 10000 and 1 <= args.workers <= 64 and 1 <= args.payload_bytes <= 60000 and 1 <= args.capacity <= 10000 and 0 < args.drain_seconds <= 120):
        parser.error('parameters exceed the bounded test envelope')
    binary = args.binary.resolve(strict=True)
    lock = threading.Lock()
    starts, latencies, end_to_end, statuses, sent = {}, [], [], Counter(), Counter()
    rejection_codes = Counter()
    next_id = 0
    rss, backlog_samples, errors = [], [], []
    rss_sampling_errors = []
    stop_sample = threading.Event()

    def receiver(kind):
        class Receiver(BaseHTTPRequestHandler):
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                if kind == 'slow':
                    time.sleep(0.05)
                status = 503 if kind == 'failure' else 202
                with lock:
                    sent[kind] += 1
                    if status == 202:
                        key = body['parts'][0]['value']['source_event_id']
                        end_to_end.append((time.monotonic()-starts[key])*1000)
                self.send_response(status)
                self.send_header('Content-Length', '0')
                self.end_headers()
            def log_message(self, *_):
                pass
        return Receiver

    with tempfile.TemporaryDirectory(prefix='synquay-load-') as directory, ExitStack() as stack:
        servers = [stack.enter_context(ThreadingHTTPServer(('127.0.0.1', 0), receiver(kind))) for kind in ['fast', args.scenario]]
        for server in servers:
            threading.Thread(target=server.serve_forever, daemon=True).start()
        cfg = json.loads((ROOT/'examples/pipeline.example.json').read_text())
        cfg.update(database=str(Path(directory)/'load.db'), destination_url=f'http://127.0.0.1:{servers[0].server_port}/',
                   admin_token_env='SYNQUAY_LOAD_ADMIN', operator_principal_id='00000000-0000-0000-0000-000000000090',
                   scheduler=dict(max_in_flight=8, min_interval_ms=0, failure_threshold=3, cooldown_ms=30000, strict_order=False),
                   additional_targets=[dict(connection_id='00000000-0000-0000-0000-000000000030', destination_id='00000000-0000-0000-0000-000000000040', rule_id='00000000-0000-0000-0000-000000000060', url=f'http://127.0.0.1:{servers[1].server_port}/', allow_loopback_http=True)])
        cfg['capacity']['max_pending_items'] = args.capacity
        cfg_path = Path(directory)/'config.json'
        cfg_path.write_text(json.dumps(cfg))
        token, admin = secrets.token_hex(32), secrets.token_hex(32)
        env = dict(os.environ, SYNQUAY_PIPELINE_CONFIG=str(cfg_path), SYNQUAY_WEBHOOK_TOKEN=token,
                   SYNQUAY_LOAD_ADMIN=admin, SYNQUAY_BIND='127.0.0.1:0', SYNQUAY_LOG_FORMAT='json', RUST_LOG='info')
        process = subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        ready = queue.Queue()
        def logs():
            for line in process.stderr:
                try:
                    event = json.loads(line)
                    fields = event.get('fields', {})
                    if fields.get('event') == 'server.started':
                        ready.put(fields['address'])
                    if event.get('level') == 'ERROR' and not (fields.get('event') == 'http.request.completed' and fields.get('status') == 503):
                        errors.append(fields.get('event', 'server_error'))
                except ValueError:
                    errors.append('non_json_log')
        log_thread = threading.Thread(target=logs, daemon=True)
        log_thread.start()
        sampler = None
        try:
            base = 'http://'+ready.get(timeout=15)
            def ops():
                with urlopen(Request(base+'/api/ops', headers={'Authorization':'Bearer '+admin}), timeout=10) as response:
                    return json.load(response)
            def sample():
                while not stop_sample.is_set():
                    try:
                        value = subprocess.check_output(['ps','-o','rss=','-p',str(process.pid)],text=True).strip()
                        if value:
                            rss.append(int(value)*1024)
                    except (OSError, ValueError, subprocess.SubprocessError) as e:
                        if not rss_sampling_errors:
                            rss_sampling_errors.append(type(e).__name__)
                    try:
                        snapshot = ops()
                        backlog_samples.append(snapshot['database']['pending_items'])
                    except (OSError, ValueError):
                        pass
                    stop_sample.wait(0.2)
            sampler = threading.Thread(target=sample, daemon=True)
            sampler.start()
            start = time.monotonic()
            def worker():
                nonlocal next_id
                while True:
                    with lock:
                        if next_id >= args.messages:
                            return
                        i = next_id
                        next_id += 1
                        key = f'load-{i}'
                        starts[key] = time.monotonic()
                    body = json.dumps({'source_event_id':key,'event_type':'load','payload':'x'*args.payload_bytes}).encode()
                    request = Request(base+'/api/webhooks/'+cfg['source_connection_id'], data=body,
                                      headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
                    begin = time.monotonic()
                    try:
                        with urlopen(request, timeout=10) as response:
                            response.read(8192)
                            status = str(response.status)
                    except HTTPError as e:
                        status = str(e.code)
                        error = json.loads(e.read(8192)).get('error', {})
                        code = error.get('code', 'unknown') if isinstance(error, dict) else str(error)
                        with lock:
                            rejection_codes[code] += 1
                        e.close()
                    except OSError:
                        status = 'transport_error'
                    with lock:
                        statuses[status] += 1
                        latencies.append((time.monotonic()-begin)*1000)
            workers = [threading.Thread(target=worker) for _ in range(args.workers)]
            for thread in workers:
                thread.start()
            for thread in workers:
                thread.join()
            ingress_elapsed = time.monotonic()-start
            deadline = time.monotonic()+args.drain_seconds
            while True:
                final = ops()
                if final['database']['pending_items'] == 0 or time.monotonic() >= deadline:
                    break
                time.sleep(0.2)
            total_elapsed = time.monotonic()-start
            stop_sample.set()
            sampler.join(timeout=12)
            process.terminate()
            assert process.wait(timeout=20) == 0, 'graceful drain failed'
            log_thread.join(timeout=2)
            def percentiles(values):
                values = sorted(values)
                return {f'p{p}_ms':values[min(len(values)-1, int((len(values)-1)*p/100))] if values else None for p in [50,95,99]}
            report = dict(scenario=args.scenario, messages=args.messages, workers=args.workers, payload_bytes=args.payload_bytes,
                          capacity=args.capacity, platform=platform.platform(), binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                          ingress_seconds=ingress_elapsed, total_seconds_including_drain=total_elapsed,
                          request_statuses=dict(statuses), rejection_codes=dict(rejection_codes), ingress_requests_per_second=args.messages/ingress_elapsed,
                          accepted_per_second=statuses['202']/ingress_elapsed, target_calls=dict(sent),
                          ingress_latency=percentiles(latencies), accepted_delivery_latency=percentiles(end_to_end),
                          rss_peak_bytes=max(rss,default=None), rss_samples=len(rss), rss_sampling_errors=rss_sampling_errors, sampled_backlog_peak=max(backlog_samples,default=0),
                          final=final, server_errors=errors,
                          limitations='Finite local scenario; not a sustained-memory or production-throughput guarantee. No retries of rejected inputs. Delivery percentiles include successful targets only; pending work and errors are reported separately.')
            assert sum(statuses.values()) == args.messages
            assert report['sampled_backlog_peak'] <= args.capacity
            assert not errors, errors
            assert set(rejection_codes) <= {'capacity_exceeded', 'ingress_busy'}, rejection_codes
            if args.scenario != 'failure':
                assert final['database']['pending_items'] == 0, 'drain timeout'
                assert sum(sent.values()) == 2*statuses['202'], 'accepted work missing or duplicated'
            else:
                assert sent['failure'] <= 3, 'circuit did not stop failure target'
                assert sent['fast'] == statuses['202'], 'healthy target starved'
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
            print(json.dumps({k:report[k] for k in ['scenario','request_statuses','target_calls','rss_peak_bytes','sampled_backlog_peak','ingress_latency']}))
        finally:
            stop_sample.set()
            if sampler:
                sampler.join(timeout=12)
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stderr.close()
            for server in servers:
                server.shutdown()

if __name__ == '__main__':
    main()
