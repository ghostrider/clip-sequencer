import json, time, urllib.request
B='http://127.0.0.1:8080/api/v1'
def req(method, path, body=None, ctype='text/plain'):
    data = None if body is None else (json.dumps(body) if ctype=='application/json' else body).encode()
    r = urllib.request.Request(B+path, data=data, method=method, headers={'Content-Type': ctype} if data else {})
    with urllib.request.urlopen(r) as resp: return resp.status
def snap():
    c = json.load(urllib.request.urlopen(B+'/composition'))
    deck = [d['name']['value'] for d in c['decks'] if d['selected']['value']][0]
    layers = [(l['id'], l['name']['value']) for l in c['layers']]
    cols = [x['id'] for x in c['columns']]
    grid = {}
    for li,l in enumerate(c['layers'],1):
        for ci,cl in enumerate(l['clips'],1):
            grid[(li,ci)] = (cl['id'], cl['connected']['value'], cl['name']['value'][:18])
    return dict(deck=deck, layers=layers, cols=cols, grid=grid)
def show(tag, s, base):
    print(f'--- {tag}  deck={s["deck"]}')
    print('  layers:', [(i+1, lid, n) for i,(lid,n) in enumerate(s['layers'])])
    print('  columns:', len(s['cols']), 'first ids', s['cols'][:4])
    content = {k:v for k,v in s['grid'].items() if v[1]!='Empty'}
    print('  content clips (layer,col)->(id,name):', {k:(v[0],v[2]) for k,v in sorted(content.items())})
    # what happened to ids of slots compared with base
    moved = []
    bid = {v[0]:k for k,v in base['grid'].items()}
    for k,v in sorted(s['grid'].items()):
        if v[0] in bid and bid[v[0]] != k: moved.append((v[0], bid[v[0]], '->', k))
        if v[0] not in bid: moved.append((v[0], 'NEW id at', k))
    print('  ids that changed position vs baseline:', moved[:12], '...' if len(moved)>12 else '')
def step(tag, method, path, body=None, ctype='text/plain'):
    st = req(method, path, body, ctype); time.sleep(0.4); s = snap(); print(f'\n### {tag}: {method} {path} {body!r} -> {st}'); return s
base = snap(); show('baseline', base, base)
s = step('swap L2C1<->L2C3 (both content)', 'POST', '/composition/clips/swap', ['/composition/layers/2/clips/1','/composition/layers/2/clips/3'], 'application/json'); show('after', s, base)
step('undo', 'POST', '/composition/clips/swap', ['/composition/layers/2/clips/1','/composition/layers/2/clips/3'], 'application/json')
s = step('swap L2C1<->L2C2 (content<->empty)', 'POST', '/composition/clips/swap', ['/composition/layers/2/clips/1','/composition/layers/2/clips/2'], 'application/json'); show('after', s, base)
step('undo', 'POST', '/composition/clips/swap', ['/composition/layers/2/clips/1','/composition/layers/2/clips/2'], 'application/json')
s = step('columns swap 1<->3', 'POST', '/composition/columns/swap', ['/composition/columns/1','/composition/columns/3'], 'application/json'); show('after', s, base)
step('undo', 'POST', '/composition/columns/swap', ['/composition/columns/1','/composition/columns/3'], 'application/json')
s = step('insert-move column 3 before column 1', 'POST', '/composition/columns/1/insert-move', '/composition/columns/3'); show('after', s, base)
step('undo: move column 1 before column 4', 'POST', '/composition/columns/4/insert-move', '/composition/columns/1')
s = step('add column before column 1', 'POST', '/composition/columns/add', '/composition/columns/1'); show('after', s, base)
step('undo: delete column 1', 'DELETE', '/composition/columns/1')
s = step('add layer before layer 1', 'POST', '/composition/layers/add', '/composition/layers/1'); show('after', s, base)
step('undo: delete layer 1', 'DELETE', '/composition/layers/1')
s = step('move layer 3 before layer 2', 'POST', '/composition/layers/3/move', '/composition/layers/2'); show('after', s, base)
step('undo: move layer 2 before layer 4', 'POST', '/composition/layers/2/move', '/composition/layers/4')
empty_id = base['grid'][(3,5)][0]
s = step('open Blank source into empty slot L3C5', 'POST', f'/composition/clips/by-id/{empty_id}/open', 'source:///video/Blank'); show('after', s, base)
print('  L3C5 id before', empty_id, 'after', s['grid'][(3,5)])
step('undo: clear L3C5', 'POST', f'/composition/clips/by-id/{empty_id}/clear')
final = snap(); show('final', final, base)
print('\nFINAL == BASELINE:', final == base)
