#!/usr/bin/python3
"""Deterministic offline UCI fixture; not used by the application at runtime."""
import pathlib
import sys
import time

mode = pathlib.Path(sys.argv[0]).stem
log_path = pathlib.Path(str(sys.argv[0]) + '.log')
multipv = 3
black = False
position_ply = 0
active = False

def emit(text):
    if mode == 'partial' and text in ('uciok', 'readyok'):
        sys.stdout.write(text[:2])
        sys.stdout.flush()
        time.sleep(0.01)
        sys.stdout.write(text[2:] + '\n')
        sys.stdout.flush()
    else:
        print(text, flush=True)

for raw in sys.stdin:
    line = raw.strip()
    with log_path.open('a') as log:
        log.write(line + '\n')
    if line == 'uci':
        if mode == 'eof':
            break
        if mode == 'no-uci':
            continue
        emit('id name Mock UCI 17.1')
        emit('option name Threads type spin default 1 min 1 max 8')
        if mode != 'no-hash':
            emit('option name Hash type spin default 16 min 16 max 512')
        emit('option name MultiPV type spin default 1 min 1 max 3')
        emit('option name UCI_AnalyseMode type check default false')
        if mode == 'batch':
            emit('option name Clear Hash type button')
        emit('uciok')
    elif line.startswith('setoption name MultiPV value '):
        multipv = int(line.split()[-1])
    elif line == 'isready':
        if mode != 'no-ready':
            emit('readyok')
    elif line.startswith('position fen '):
        black = ' b ' in line
        if mode == 'batch':
            position_ply = len(line.split(' moves ')[1].split()) if ' moves ' in line else 0
            black = position_ply % 2 == 1
    elif line.startswith('go '):
        if mode == 'search-eof':
            break
        if mode == 'flood':
            while True:
                emit('info string bounded-output stress fixture')
        if mode == 'invalid-utf8':
            sys.stdout.buffer.write(b'\xff\n')
            sys.stdout.flush()
            continue
        if mode == 'oversized':
            emit('x' * 66000)
            continue
        if mode == 'malformed':
            emit('info depth broken score cp NaN pv bad')
        active = True
        candidates = ['e7e5', 'c7c5', 'e7e6'] if black else ['e2e4', 'd2d4', 'g1f3']
        depths = (1, 2)
        if mode == 'batch':
            candidates = [['e2e4', 'd2d4', 'g1f3'], ['e7e5', 'c7c5', 'e7e6'], ['g1f3', 'f1c4', 'd2d4']][position_ply]
            depths = (int(line.split()[2]),)
        for depth in depths:
            for index in range(multipv):
                emit(f'info depth {depth} seldepth {depth+2} multipv {index+1} score cp {25-index*10} nodes 123 nps 1000 time 12 pv {candidates[index]}')
        if mode not in ('hold', 'no-bestmove'):
            emit('bestmove ' + candidates[0])
            active = False
    elif line == 'stop':
        if active and mode != 'no-bestmove':
            emit('bestmove ' + ('e7e5' if black else 'e2e4'))
            active = False
    elif line == 'quit':
        if mode != 'no-quit':
            break
