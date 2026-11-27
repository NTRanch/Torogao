import sys

path, want = sys.argv[1], int(sys.argv[2])
lines = open(path).read().splitlines()
n = int(lines[2].split()[1])
i = 3
types = []
for idx in range(n):
    t = lines[i].split()
    a = lines[i + 1]
    nn = lines[i + 2]
    i += 3
    types.append((t, a.split(" ", 2)[2], nn.split(" ", 2)[2]))
for idx in range(n):
    d = lines[i].split()
    i += 1
    cnt = int(d[2])
    fl = lines[i : i + cnt]
    i += cnt
    if idx == want:
        t, asm, nm = types[idx]
        print(idx, nm, "asm=" + asm, "T:", " ".join(t))
        for ln in fl:
            print("   ", ln)
