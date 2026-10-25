import re
import sys
from collections import OrderedDict, defaultdict
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
EPICS = ROOT / "_bmad-output/planning-artifacts/epics.md"
STATUS = ROOT / "_bmad-output/implementation-artifacts/sprint-status.yaml"
OUT = ROOT / "_bmad-output/planning-artifacts/kanban-board.md"

LANES = OrderedDict([
    ("CP", ("Control plane", "Cloud API, auth, wizard (onboarding web), licence, mailbox, presign, rollup ingest, Razorpay, messaging adapter, admin and partner backends",
        ["2.10", "2.11", "2.12", "2.13", "2.14", "2.15", "2.16", "2.19", "2.22",
         "2.28", "2.29", "2.30", "2.31", "6.1",
         "8.1", "8.5", "8.7", "8.9", "8.18", "8.20", "8.28", "8.3", "8.35", "8.25", "8.36", "8.37",
         "9.3", "9.15",
         "10.1", "10.2", "10.3", "10.4", "10.5", "10.6", "10.7", "10.9", "10.10", "10.11", "10.14", "10.15", "10.20",
         "11.9", "11.8", "11.11", "11.13", "11.14", "11.15", "11.17", "11.18", "11.19", "11.21", "11.31",
         "13.29", "13.28",
         "14.17", "14.24", "14.25", "14.26", "14.27", "14.28", "14.35", "14.42", "14.53"])),
    ("HUB-A", ("Hub floor and sessions", "Windows Hub: devices, floor, players, sessions, wallet, credit, plans, pricing, catalogue match, bookings, tournaments, loyalty, vault, imports, back office",
        ["2.20", "2.21", "2.23", "2.24", "2.25",
         "3.1", "3.2", "3.3", "3.4", "3.5", "6.2", "6.3", "6.5",
         "3.6", "3.7", "3.8", "3.9", "3.11", "3.12", "3.13", "3.14", "3.15", "3.16", "3.17", "3.18", "3.19", "3.20",
         "3.21", "3.22", "3.23", "3.24", "3.25", "3.26", "3.27", "3.28", "3.29", "3.30", "3.31", "3.32", "3.33", "3.34",
         "3.35", "3.36", "3.37",
         "13.1", "13.2", "13.3", "13.4", "13.5", "13.6", "13.7", "13.8", "13.9", "13.10", "13.11", "13.12", "13.13",
         "13.14", "13.15", "13.16", "13.18", "13.23", "13.24", "13.25", "13.26", "13.27", "13.30",
         "14.1", "14.2", "14.3", "14.4", "14.5", "14.6", "14.7", "14.8", "14.9", "14.29", "14.31", "14.33", "14.34"])),
    ("KIOSK", ("Kiosk", "Windows kiosk service and player UI",
        ["1.19", "3.10", "6.4", "6.6", "6.7", "6.8", "6.9", "6.10", "6.11", "6.12", "6.13", "6.14", "6.15", "6.16",
         "6.17", "6.18", "6.19", "6.20", "6.21", "6.22", "6.23", "6.24", "6.25", "6.26", "6.27", "6.28", "6.29",
         "6.30", "6.31", "6.32", "6.33", "6.34", "6.35", "6.36", "6.37", "6.38",
         "13.17", "13.19", "13.20", "13.21", "13.22", "11.33"])),
    ("HUB-B", ("Hub POS and money", "Windows Hub: tax, products, stock, POS, shifts, receipts, printing, reports, CSV import, setup templates",
        ["5.1", "5.2", "5.3", "5.4", "5.5", "5.6", "5.7", "5.8", "5.9", "5.10", "5.11", "5.12", "5.13", "5.14",
         "5.15", "5.16", "5.17", "5.18", "5.19", "5.20", "5.21", "5.22", "5.23", "5.24", "5.25", "5.26", "5.27",
         "5.28", "5.29", "5.30", "5.31", "5.32", "5.33", "5.34", "5.35", "5.36", "5.37", "5.38", "5.39"])),
    ("OWNER", ("Owner app", "Owner PWA: shell, dashboards, reports, commands, alerts, audit, live view, subscription, team",
        ["8.10", "8.11", "8.12", "8.14", "8.13", "8.15", "8.16", "8.17", "8.21", "8.22", "8.23", "8.24", "8.26",
         "8.27", "8.32", "8.33", "8.38", "8.40", "8.42", "9.8", "9.10", "9.17", "10.8",
         "14.18", "14.19", "14.20", "14.21", "14.22", "14.23", "14.30"])),
    ("HUB-C", ("Hub platform", "Windows Hub: cloud sync, outbox, alerts, Tunnel, backups, restore, standby and promote, diagnostics, updates, wasm demo",
        ["8.2", "8.4", "8.6", "8.8", "8.19", "8.29", "8.30", "8.31", "8.34", "8.39", "8.41",
         "9.1", "9.2", "9.4", "9.5", "9.6", "9.7", "9.9", "9.11", "9.12", "9.13", "9.14", "9.16",
         "9.18", "9.19", "9.20", "9.21", "9.22", "9.23", "9.24", "9.25", "9.26", "9.27",
         "10.12", "10.13", "11.28", "11.29",
         "14.13", "14.14", "14.32", "14.36", "14.37", "14.38", "14.39", "14.40", "14.41",
         "14.15", "14.16", "14.43", "14.44", "14.45", "14.47", "14.48", "14.49", "14.50"])),
    ("KITCHEN", ("Kitchen", "Hub kitchen tickets, KOT printing and the kitchen display app",
        ["7.1", "7.2", "7.3", "7.4", "7.5", "7.6", "7.7", "7.8", "7.9", "7.10", "7.11", "14.10", "14.11", "14.12"])),
    ("ANDROID", ("Android Hub", "Tauri mobile Hub, ads, Free limits, Play rollout, migration to Windows",
        ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6", "4.7", "4.9", "4.8", "4.10", "4.11", "4.12", "4.13", "4.14",
         "4.15", "4.16", "4.17", "10.16", "10.17", "10.18", "10.19"])),
    ("WEB", ("Marketing, admin and partner", "Astro marketing site, admin app and partner portal front ends",
        ["11.1", "11.2", "11.3", "11.4", "11.5", "11.6", "11.7", "11.10", "11.12", "11.16", "11.20", "11.22",
         "11.23", "11.24", "11.25", "11.26", "11.27", "11.30", "11.32", "14.51", "14.52", "14.54"])),
    ("PI", ("Console TVs", "Pi image and agent, TV control, QR self-start",
        ["12.1", "12.2", "12.3", "12.4", "12.5", "12.6", "12.7", "12.8", "12.9", "12.10", "12.11", "12.12",
         "12.13", "12.14", "12.15", "12.16", "14.46"])),
])

GATES = {
    "2.28": ["2.20", "2.21", "2.23", "2.24", "2.25", "2.22"],
    "2.29": ["2.28", "3.9"], "2.31": ["2.29", "2.30"],
    "8.1": ["2.31"], "8.5": ["3.12", "5.11"], "8.7": ["8.5"], "8.20": ["8.18"], "9.15": ["8.8"], "8.9": ["8.4", "8.6"], "8.18": ["2.15"], "8.3": ["8.2"],
    "8.35": ["8.4"], "8.36": ["8.2"], "8.37": ["8.36"], "8.25": ["8.18"],
    "10.4": ["10.3"], "10.6": ["10.3", "2.12"], "10.7": ["10.3"], "10.9": ["10.3"], "10.10": ["10.3"],
    "10.11": ["10.3"], "10.14": ["10.11"], "10.15": ["10.3"],
    "11.8": ["11.1"], "11.13": ["11.11", "8.28"], "11.14": ["11.13"], "11.15": ["10.3"], "11.17": ["11.15", "10.3"],
    "11.18": ["11.17"], "11.19": ["11.18", "11.14"], "11.21": ["11.18"], "11.31": ["11.8"],
    "13.29": ["13.11", "13.8", "13.2"], "13.28": ["8.18", "13.11", "13.14"],
    "14.17": ["14.15", "14.16"], "14.24": ["14.17", "8.28"], "14.25": ["10.9"], "14.26": ["14.25"],
    "14.27": ["14.26"], "14.28": ["14.27"], "14.53": ["2.3"],
    "2.20": ["2.18"], "2.25": ["2.20"], "6.2": ["6.1"], "6.3": ["6.2"], "6.5": ["6.3", "6.4"],
    "3.9": ["3.7", "2.25", "3.1"], "3.12": ["3.11"], "3.22": ["3.21"], "3.24": ["3.22", "3.23"],
    "3.35": ["2.21"], "13.1": ["3.9"], "13.11": ["3.12"], "13.16": ["9.2"], "13.18": ["13.16", "13.17"],
    "13.23": ["13.22"], "13.25": ["13.24"], "13.26": ["13.24"], "13.27": ["13.25", "13.26", "5.3", "3.21"],
    "14.1": ["5.5"], "14.3": ["5.5"], "14.8": ["5.10"], "14.29": ["5.24"], "14.31": ["14.30"],
    "14.33": ["8.19", "3.29"], "14.34": ["14.33"],
    "3.10": ["3.9", "1.19"], "6.4": ["2.18"], "6.6": ["6.2"], "6.7": ["3.30"], "6.8": ["3.6", "3.9"],
    "6.9": ["3.24", "6.8"], "6.10": ["3.6", "6.8"], "6.13": ["6.8"], "6.14": ["6.13", "3.23"],
    "6.16": ["3.5"], "6.18": ["3.11"], "6.19": ["3.23"], "6.20": ["6.19"], "6.21": ["3.28"],
    "6.22": ["3.26", "6.21"], "6.23": ["6.3", "6.5"], "6.26": ["6.23"], "6.27": ["6.23"],
    "6.30": ["5.9"], "6.31": ["5.12", "6.30"], "6.32": ["3.16", "6.31"], "6.33": ["7.3"],
    "6.34": ["3.16", "3.22"], "6.36": ["3.23"], "6.38": ["6.37"],
    "13.17": ["13.16"], "13.19": ["13.17"], "13.20": ["13.19"], "13.21": ["13.20"], "13.22": ["6.23"],
    "11.33": ["6.38"],
    "5.9": ["3.13", "5.3"], "5.10": ["3.12"], "5.11": ["5.9", "5.10"], "5.12": ["5.11", "3.9"],
    "5.16": ["5.14", "5.2"], "5.17": ["5.16", "2.27"], "5.19": ["5.15", "8.29"], "5.26": ["5.10", "5.25"],
    "5.28": ["5.26", "5.27"], "5.30": ["3.17"], "5.33": ["5.3"], "5.34": ["5.33", "3.21"],
    "5.37": ["5.36", "8.1"], "5.38": ["5.37", "2.30"], "5.39": ["5.38"],
    "8.10": ["2.6"], "8.14": ["2.31"], "8.13": ["8.9"], "8.15": ["8.9"], "8.16": ["8.9"],
    "8.17": ["8.15", "8.16", "5.28"], "8.21": ["8.18", "8.19"], "8.22": ["8.21"], "8.23": ["8.21", "3.29", "5.21"],
    "8.24": ["8.21", "5.4"], "8.26": ["8.21", "2.16"], "8.27": ["8.21", "2.24"], "8.32": ["8.31", "8.9"],
    "8.33": ["8.28", "8.34", "8.35"], "8.38": ["8.37"], "8.40": ["8.39"], "8.42": ["8.40", "8.33"],
    "9.8": ["9.3"], "9.10": ["9.9", "9.2"], "9.17": ["9.16"], "10.8": ["10.7", "10.10"],
    "14.18": ["14.17"], "14.19": ["14.17", "14.14"], "14.20": ["14.17"], "14.21": ["14.17"],
    "14.22": ["14.14", "8.35"], "14.23": ["14.24"], "14.30": ["14.29"],
    "8.2": ["8.1"], "8.4": ["8.2"], "8.6": ["8.5"], "8.8": ["8.2"], "8.19": ["8.18"], "8.29": ["8.28", "3.14"],
    "8.30": ["8.29", "3.23"], "8.31": ["8.29", "3.20"], "8.34": ["8.29", "5.26", "5.8", "5.24"],
    "8.39": ["2.12"], "8.41": ["8.39"],
    "9.1": ["3.6"], "9.2": ["9.1", "8.1"], "9.4": ["5.32"], "9.5": ["9.4", "2.26"], "9.6": ["9.4"],
    "9.7": ["9.4", "9.3"], "9.9": ["9.8"], "9.11": ["8.8", "10.6"], "9.12": ["9.11", "9.9"],
    "9.13": ["9.12", "9.7"], "9.14": ["5.29"], "9.16": ["8.8"],
    "9.18": ["9.8"], "9.19": ["9.18"], "9.20": ["9.19"], "9.21": ["9.20", "9.3"], "9.22": ["9.21"],
    "9.23": ["9.21", "9.13"], "9.24": ["9.21"], "9.25": ["9.24"], "9.26": ["9.25"], "9.27": ["9.13", "9.1"],
    "10.12": ["10.11", "2.19"], "10.13": ["10.12"], "11.28": ["2.11"], "11.29": ["11.28"],
    "14.13": ["12.2"], "14.14": ["14.13"], "14.32": ["8.19", "6.16"],
    "14.36": ["14.35", "9.18"], "14.37": ["14.36"], "14.38": ["14.37"], "14.39": ["14.38", "9.21"],
    "14.40": ["14.39", "8.41"], "14.41": ["14.40"],
    "14.15": ["8.7", "13.29", "14.14"], "14.16": ["14.15"], "10.20": ["10.19", "10.15", "10.14"],
    "14.43": ["14.42"], "14.44": ["14.43", "9.19"], "14.45": ["14.43"], "14.47": ["14.45"],
    "14.49": ["14.48"], "14.50": ["14.49"],
    "7.1": ["2.26"], "7.2": ["5.12", "6.31"], "7.3": ["7.2", "5.3"], "7.4": ["7.3", "3.25"], "7.5": ["7.3", "5.17"],
    "7.6": ["7.3"], "7.7": ["7.6"], "7.8": ["7.7"], "7.9": ["7.7"], "7.10": ["7.8"], "7.11": ["7.10"],
    "14.10": ["7.11"], "14.11": ["14.10"], "14.12": ["14.10"],
    "4.1": ["1.22"], "4.3": ["2.10", "2.11"], "4.4": ["4.3"], "4.6": ["5.17"], "4.7": ["9.5"],
    "4.9": ["2.12", "2.19"], "4.8": ["2.31", "4.4", "4.9"], "4.14": ["4.10"], "4.15": ["10.6", "4.10"],
    "4.16": ["4.8"], "4.17": ["4.16"], "10.16": ["10.6", "9.4"], "10.17": ["10.16"], "10.18": ["10.17", "9.7"],
    "10.19": ["10.18"],
    "11.1": ["1.4"], "11.10": ["11.9"], "11.12": ["11.3"], "11.16": ["11.17"], "11.20": ["11.18", "11.19"],
    "11.22": ["11.10", "6.1"], "11.23": ["11.22"], "11.24": ["11.23"], "11.25": ["11.10", "10.15"],
    "11.26": ["11.10", "10.3"], "11.27": ["11.10", "8.9", "8.36"], "11.30": ["11.10", "11.28"],
    "11.32": ["11.21", "11.26"], "14.51": ["14.50"], "14.54": ["14.42"],
    "12.2": ["12.1"], "12.4": ["12.2", "2.18"], "12.5": ["12.4", "3.35"], "12.6": ["12.5", "3.30"],
    "12.7": ["12.6", "3.24"], "12.8": ["12.7", "3.5"], "12.9": ["12.8", "12.3"], "12.10": ["12.9"],
    "12.11": ["12.9", "3.36"], "12.12": ["12.11"], "12.13": ["12.3", "3.5"], "12.14": ["12.12"],
    "12.15": ["12.2"], "12.16": ["12.15", "12.14"], "14.46": ["14.45", "12.14"],
}

PULL_ORDER = ["CP", "HUB-A", "KIOSK", "HUB-B", "OWNER", "HUB-C", "KITCHEN", "ANDROID", "WEB", "PI"]
LANDED = {"review", "done"}
V11_EPICS = {"12", "13", "14"}


def load_stories():
    titles = OrderedDict()
    for m in re.finditer(r"^### Story (\d+\.\d+): (.+)$", EPICS.read_text(), re.M):
        titles[m.group(1)] = m.group(2).strip()
    return titles


def load_status():
    keys, status = {}, {}
    for m in re.finditer(r"^  (\d+)-(\d+)-(\S+): ([a-z-]+)\s*$", STATUS.read_text(), re.M):
        num = f"{m.group(1)}.{m.group(2)}"
        keys[num] = f"{m.group(1)}-{m.group(2)}-{m.group(3)}"
        status[num] = m.group(4)
    return keys, status


def main():
    titles = load_stories()
    keys, status = load_status()
    assigned = [s for _, (_, _, lst) in LANES.items() for s in lst]
    dupes = {s for s in assigned if assigned.count(s) > 1}
    if dupes:
        sys.exit(f"duplicate assignments: {sorted(dupes)}")
    landed_e1_e2 = [s for s in titles if s not in assigned]
    missing = [s for s in landed_e1_e2 if status.get(s) not in LANDED]
    if missing:
        sys.exit(f"unassigned stories that are not landed: {missing}")
    unknown = [s for s in assigned if s not in titles]
    if unknown:
        sys.exit(f"assigned stories not in epics.md: {unknown}")
    for s, gates in GATES.items():
        bad = [g for g in gates if g not in titles]
        if bad:
            sys.exit(f"gate on unknown story {bad} for {s}")

    lane_of = {s: lane for lane, (_, _, lst) in LANES.items() for s in lst}

    def is_landed(s):
        return status.get(s) in LANDED

    def blockers(s):
        return [g for g in GATES.get(s, []) if not is_landed(g)]

    def predecessor_open(lane, s):
        lst = LANES[lane][2]
        i = lst.index(s)
        return [p for p in lst[:i] if status.get(p) not in LANDED | {"in-progress"}]

    out = []
    out.append("---")
    out.append(f"generated: {date.today().isoformat()}")
    out.append("source_of_truth_for_status: _bmad-output/implementation-artifacts/sprint-status.yaml")
    out.append("source_of_truth_for_scope: _bmad-output/planning-artifacts/epics.md")
    out.append("regenerate: uv run _bmad-output/planning-artifacts/kanban/build_board.py")
    out.append("---")
    out.append("")
    out.append("# Arena 360 Kanban board")
    out.append("")
    out.append("Every story below is one of the 417 stories in `epics.md`, unchanged. This file only decides the order in which they are pulled. Status is read from `sprint-status.yaml` at generation time, so re-run the script after each landing.")
    out.append("")
    out.append("## Pull rules")
    out.append("")
    out.append("1. Lanes run in parallel. The global WIP limit is 4 implementers; at most one story in progress per lane, except CP which may hold two.")
    out.append(f"2. When a slot frees, walk the lanes in priority order ({' > '.join(PULL_ORDER)}) and pull the first story that is **pullable**: every earlier story in its lane is committed (`in-progress` counts, since story N+1 branches from N's WIP commit as the overlap rule allows), every cross-lane gate in **Waits on** is `review` or `done`, and the lane is under its WIP limit.")
    out.append("3. A story from Epics 12 to 14 (v1.1) is pulled only when no lane has a pullable MVP story.")
    out.append("4. Proof stories (titles containing \"proven\") close a lane's block and need everything before them in the lane landed.")
    out.append("5. Status is never written here. The build loop keeps updating `sprint-status.yaml` exactly as before; this file is regenerated from it.")
    out.append("6. Hub migrations stay contiguous: a Hub story that lands after a sibling renumbers its migration on rebase (rule from the loop state).")
    out.append("")

    counts = defaultdict(lambda: defaultdict(int))
    for lane, (_, _, lst) in LANES.items():
        for s in lst:
            counts[lane][status.get(s, "backlog")] += 1

    out.append("## Lane summary")
    out.append("")
    out.append("| Lane | Scope | Stories | Landed | In progress | Backlog |")
    out.append("|---|---|---:|---:|---:|---:|")
    for lane, (name, _, lst) in LANES.items():
        c = counts[lane]
        landed = c["review"] + c["done"]
        out.append(f"| {lane} | {name} | {len(lst)} | {landed} | {c['in-progress']} | {c['backlog'] + c['ready-for-dev']} |")
    out.append(f"| LANDED | Epic 1 and 2 stories already shipped and not re-listed | {len(landed_e1_e2)} | {len(landed_e1_e2)} | 0 | 0 |")
    out.append("")

    out.append("## Pull next")
    out.append("")
    out.append("Candidates in priority order. Take the top ones until the 4 implementer slots are full.")
    out.append("")
    pullable = []
    for lane in PULL_ORDER:
        if any(status.get(s) == "in-progress" for s in LANES[lane][2]) and lane != "CP":
            continue
        for s in LANES[lane][2]:
            if is_landed(s) or status.get(s) == "in-progress":
                continue
            if not predecessor_open(lane, s) and not blockers(s):
                pullable.append((lane, s))
            break
    mvp = [(l, s) for l, s in pullable if s.split(".")[0] not in V11_EPICS]
    v11 = [(l, s) for l, s in pullable if s.split(".")[0] in V11_EPICS]
    for lane, s in mvp + (v11 if not mvp else []):
        out.append(f"- **{lane}** {s} {titles[s]} (`{keys.get(s, '?')}`)")
    if not pullable:
        out.append("- nothing is pullable; every lane is blocked or in progress")
    out.append("")
    out.append("In flight now:")
    out.append("")
    for s, st in status.items():
        if st == "in-progress":
            out.append(f"- **{lane_of.get(s, 'LANDED')}** {s} {titles.get(s, '?')}")
    out.append("")

    for lane, (name, scope, lst) in LANES.items():
        out.append(f"## {lane}: {name}")
        out.append("")
        out.append(scope + ".")
        out.append("")
        if lane == "CP":
            out.append("After 2.31 and before 6.1, finish Story 2.3's deferred observability ACs (Sentry and the OpenTelemetry/Grafana export), as the loop state records.")
            out.append("")
        out.append("| # | Story | Title | Phase | Status | Waits on |")
        out.append("|---:|---|---|---|---|---|")
        for i, s in enumerate(lst, 1):
            phase = "v1.1" if s.split(".")[0] in V11_EPICS else "MVP"
            gates = GATES.get(s, [])
            waits = ", ".join(f"~~{g}~~" if is_landed(g) else g for g in gates)
            out.append(f"| {i} | {s} | {titles[s]} | {phase} | {status.get(s, 'backlog')} | {waits} |")
        out.append("")

    out.append("## LANDED: Epic 1 and 2 stories already shipped")
    out.append("")
    out.append("| Story | Title | Status |")
    out.append("|---|---|---|")
    for s in landed_e1_e2:
        out.append(f"| {s} | {titles[s]} | {status.get(s)} |")
    out.append("")
    OUT.write_text("\n".join(out))
    print(f"wrote {OUT.relative_to(ROOT)}: {len(assigned)} assigned + {len(landed_e1_e2)} landed = {len(titles)} stories")


if __name__ == "__main__":
    main()
