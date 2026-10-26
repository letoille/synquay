"""One-time local demo seeding through the management API, with resumable receipts."""
import json
import urllib.request
import uuid
from pathlib import Path


def seed_if_configured(data: Path, base: str):
    for name in ('demo-routes.json', 'demo-routes-v2.json'):
        seed_plan(data / name, base)


def seed_plan(path: Path, base: str):
    if not path.exists():
        return
    plan = json.loads(path.read_text())
    if plan.get('completed'):
        return

    def save():
        temporary = path.with_suffix('.tmp')
        temporary.write_text(json.dumps(plan, ensure_ascii=False, indent=2))
        temporary.replace(path)

    def request(endpoint, body=None):
        req = urllib.request.Request(base + endpoint,
            data=None if body is None else json.dumps(body).encode(),
            headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=40) as response:
            return json.load(response)

    space = request('/api/workspace')
    if space['id'] != plan['workspace_id']:
        raise RuntimeError('Demo routes workspace does not match the running console')
    view = request('/api/routes')
    if not view.get('available') or view['source_id'] != plan['source_id']:
        raise RuntimeError('Demo pipeline is not active in this console')
    for item in plan['routes']:
        if item.get('done'):
            continue
        pending = item.get('pending')
        if pending is None:
            view = request('/api/routes')
            pending = dict(operation_id=str(uuid.uuid4()), expected_revision=view['snapshot']['revision'],
                route_id=item['id'], change=dict(kind='save', name=item['name'], enabled=False,
                    event_type=item['event_type'], destination_ids=item['destination_ids']))
            for field in ('source_id', 'processor'):
                if field in item:
                    pending['change'][field] = item[field]
            item['pending'] = pending
            save()
        receipt = request('/api/routes/operations/' + pending['operation_id'])['receipt']
        if receipt is None:
            receipt = request('/api/routes', pending)
        if receipt['operation_id'] != pending['operation_id']:
            raise RuntimeError('Unexpected route operation receipt')
        item['done'] = True
        save()
    plan['completed'] = True
    save()
    print(f'Routes: {len(plan["routes"])} local demo paths saved, all stopped.', flush=True)
