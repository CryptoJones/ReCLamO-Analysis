#!/usr/bin/env python3
"""Independent long-context eval: Palisade Regional Transit Cooperative audit.

WHY A REGEX / KEYWORD / COUNTING APPROACH FAILS
- The net authorization is a ledger result (last successful restatement
  ceiling + later change-order deltas − vacated riders). That integer is
  never emitted as a single dollar figure; many nearby figures are.
- Successor identity is a paraphrase chain: a visioning working title,
  a kill notice that does not use that full title, a transfer onto an
  official name, then a rebrand. Grep on the question's title hits the
  visioning note, which does not name the successor.
- Informal mail calls the Chair the "certifying member". Bylaw III.7
  (and a later GC correction) define a different person: the eligible Yea
  with the most recent appointment. Minutes also mis-record the newest
  trustee as Yea; an erratum flips that vote.
- Members appear as roles ("the Treasurer"), nicknames, and pre-change
  surnames. The required name is the legal name at archive close, which
  may not appear in the restatement minutes.
- COI recusal inherited from the absorbed initiative, a false absorption
  rumor, a near-miss sibling project, and a pre-restatement rider that
  the Finance SOP extinguishes all punish surface matching.
"""

from __future__ import annotations

import random
import re
from dataclasses import dataclass, field
from datetime import date, timedelta
from typing import Any

TARGET_CHARS = {"small": 60_000, "medium": 300_000, "large": 1_200_000}

SCALE = {
    "small": dict(voting=11, staff=9, projects=9, extra_meetings=8, reports=2),
    "medium": dict(voting=13, staff=16, projects=20, extra_meetings=28, reports=10),
    "large": dict(voting=15, staff=24, projects=36, extra_meetings=70, reports=36),
}

GIVEN = [
    "Priya", "Margaret", "Elena", "Naomi", "Harriet", "Sylvia", "Ingrid",
    "Paula", "Renee", "Keisha", "Monica", "Alicia", "Beatrice", "Chloe",
    "Dina", "Frances", "Gita", "Helena", "Imani", "Julia", "James",
    "Marcus", "Victor", "Owen", "Samuel", "Wesley", "Andre", "Nolan",
    "Patrick", "Rafael", "Christopher", "Dominic", "Felix", "Gabriel",
    "Hugo", "Isaac", "Leo", "Jonah", "Bennett", "Caleb", "Nora", "Iris",
    "Quinn", "Ruth", "Simon", "Tessa", "Uma", "Vera", "Wade", "Yara",
]
SURNAME = [
    "Calderon", "Vos", "Okoye", "Chen", "Alvarez", "Reeve", "Haddad",
    "Bristol", "Kaur", "Okafor", "Nguyen", "Perez", "Saito", "Dwyer",
    "Lindholm", "Mbeki", "Foster", "Rahman", "Ibarra", "Macrae",
    "Quintero", "Ellison", "Yates", "Kovacs", "Bergstrom", "Nilsen",
    "Padilla", "Shah", "Takeda", "Ullman", "Vargas", "Wojcik",
    "Zimmerman", "Abbott", "Beckett", "Crowley", "Dsouza", "Eastman",
    "Farley", "Gupta", "Howell", "Inoue", "Jensen", "Kline", "Larsen",
]
NICK = {
    "Margaret": "Meg", "James": "Jim", "Frances": "Frankie",
    "Helena": "Lena", "Samuel": "Sam", "Monica": "Moni", "Renee": "Ren",
    "Victor": "Vic", "Wesley": "Wes", "Gabriel": "Gabe", "Beatrice": "Bea",
    "Harriet": "Hattie", "Sylvia": "Syl", "Christopher": "Kit",
    "Patrick": "Pat", "Rafael": "Raff", "Naomi": "Nomi", "Keisha": "Kesh",
    "Bennett": "Ben", "Theodore": "Theo", "Jordan": "Jordy",
}
STAFF_ROLES = [
    "General Manager", "Deputy GM", "General Counsel", "Recording Secretary",
    "Operations Director", "Finance Director", "Procurement Lead",
    "Communications Officer", "Safety Officer", "Grants Manager",
    "IT Manager", "Facilities Supervisor", "Scheduling Chief",
    "Customer Service Manager", "Labor Relations Lead",
]
PLACES = ["Harbor", "Lockport", "Westwharf", "Baymill", "Ridgeline", "Oakbridge"]
VEHICLES = ["Circulator", "Shuttle", "Skip-Stop", "Night Owl"]
PHASES = ["Restart", "Revival", "Relaunch"]
STREETS = [
    "Cedar", "Maple", "Wharf", "Foundry", "Quarry", "Millrace", "Linden",
    "Palisade", "Cinder", "Nettle", "Ash", "Copper", "Sycamore", "Rowan",
]
VENDORS = [
    "Nettlefield Civil LLC", "Rowan Signal Co", "Ashford Pavement",
    "Quarry Glass", "Millrace Fleet", "Cinderwell Electric",
    "Linden Shelter Works", "Palisade Asphalt Index Group",
]

ONES = (
    "zero one two three four five six seven eight nine ten eleven twelve "
    "thirteen fourteen fifteen sixteen seventeen eighteen nineteen"
).split()
TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]


def _under_1000(n: int) -> str:
    if n < 20:
        return ONES[n]
    if n < 100:
        tens, ones = divmod(n, 10)
        return TENS[tens] if ones == 0 else f"{TENS[tens]}-{ONES[ones]}"
    h, rest = divmod(n, 100)
    if rest == 0:
        return f"{ONES[h]} hundred"
    return f"{ONES[h]} hundred {_under_1000(rest)}"


def int_to_words(n: int) -> str:
    if n < 0:
        return "negative " + int_to_words(-n)
    if n < 1000:
        return _under_1000(n)
    if n < 1_000_000:
        a, b = divmod(n, 1000)
        return _under_1000(a) + " thousand" if b == 0 else f"{_under_1000(a)} thousand {_under_1000(b)}"
    a, b = divmod(n, 1_000_000)
    head = _under_1000(a) + " million"
    return head if b == 0 else f"{head} {int_to_words(b)}"


def fmt_usd(n: int) -> str:
    sign = "-" if n < 0 else ""
    return f"{sign}${abs(n):,}"


def money_phrase(rng: random.Random, n: int) -> str:
    style = rng.choice(["plain", "plain", "words", "dollars"])
    if style == "plain" or abs(n) >= 100_000:
        return fmt_usd(n)
    if style == "words":
        return int_to_words(abs(n)) + (" dollars" if n >= 0 else " dollars of reduction")
    return f"{abs(n):,} dollars" if n >= 0 else f"a reduction of {abs(n):,} dollars"


def fmt_date(rng: random.Random, d: date) -> str:
    style = rng.choice(["iso", "long", "long", "short"])
    if style == "iso":
        return d.isoformat()
    if style == "short":
        return f"{d.month}/{d.day}/{d.year}"
    return f"{d.strftime('%B')} {d.day}, {d.year}"


def _norm(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", " ", s.lower()).strip()


@dataclass
class Person:
    pid: int
    given: str
    surname: str
    role: str
    voting: bool
    appointed: date
    departed: date | None = None
    nickname: str | None = None
    prior_surname: str | None = None
    name_change_date: date | None = None
    mill_district: bool = False
    coi_projects: set[int] = field(default_factory=set)
    coi_divest_date: date | None = None
    spouse_vendor: str | None = None

    def legal_name_on(self, d: date) -> str:
        if (
            self.prior_surname
            and self.name_change_date is not None
            and d < self.name_change_date
        ):
            return f"{self.given} {self.prior_surname}"
        return f"{self.given} {self.surname}"

    def current_legal_name(self) -> str:
        return f"{self.given} {self.surname}"

    def email_local(self) -> str:
        base = (self.prior_surname or self.surname).lower().replace("-", "")
        return f"{self.given[0].lower()}{base}@prtc.example"


@dataclass
class Project:
    pid: int
    working_title: str
    official_name: str
    rebrand_name: str | None
    cancelled: bool = False
    absorbed_into: int | None = None
    vendor: str = ""
    place: str = ""
    vehicle: str = ""
    phase: str = ""


@dataclass
class Motion:
    mid: int
    meeting_date: date
    project_id: int
    kind: str
    amount: int | None
    label: str
    true_votes: dict[int, str] = field(default_factory=dict)
    recorded_votes: dict[int, str] = field(default_factory=dict)
    recused: set[int] = field(default_factory=set)
    absent: set[int] = field(default_factory=set)
    voided: bool = False
    void_date: date | None = None
    uses_name: str = ""


@dataclass
class World:
    rng_seed: int
    size: str
    people: list[Person]
    projects: list[Project]
    motions: list[Motion]
    restatement: Motion
    cancelled: Project
    successor: Project
    net_amount: int
    outcome: str
    certifying: Person
    restatement_date: date
    award_date: date
    rebrand_date: date
    visioning_date: date
    kill_date: date
    transfer_date: date
    precision_date: date
    informal_date: date
    erratum_date: date
    gc_date: date
    directory_date: date
    name_change_date: date
    pre_co: Motion
    later_cos: list[Motion]
    forbidden: set[int]
    newest: Person
    chair: Person
    treasurer: Person
    recused_member: Person
    absent_member: Person
    rumor_project: Project
    sibling: Project


def sitting(people: list[Person], d: date) -> list[Person]:
    out = []
    for p in people:
        if not p.voting:
            continue
        if p.appointed <= d and (p.departed is None or p.departed > d):
            out.append(p)
    return out


def identity_set(cancelled: Project, successor: Project) -> set[int]:
    return {cancelled.pid, successor.pid}


def has_coi(p: Person, proj_ids: set[int], meeting_date: date) -> bool:
    if not (p.coi_projects & proj_ids):
        return False
    if p.coi_divest_date is not None and p.coi_divest_date < meeting_date:
        return False
    return True


def motion_passed(m: Motion) -> bool:
    yeas = nays = 0
    for pid, v in m.true_votes.items():
        if pid in m.recused or pid in m.absent:
            continue
        if v == "yea":
            yeas += 1
        elif v == "nay":
            nays += 1
    return yeas > nays


def certifying_member(m: Motion, people: list[Person]) -> Person | None:
    by_id = {p.pid: p for p in people}
    yeas: list[Person] = []
    for pid, v in m.true_votes.items():
        if pid in m.recused or pid in m.absent or v != "yea":
            continue
        yeas.append(by_id[pid])
    if not yeas:
        return None

    def key(p: Person) -> tuple:
        name = p.legal_name_on(m.meeting_date)
        parts = name.replace("-", " ").split()
        return (p.appointed, parts[-1], parts[0])

    return max(yeas, key=key)


def unique_amount(rng: random.Random, lo: int, hi: int, step: int, forbidden: set[int]) -> int:
    for _ in range(400):
        n = rng.randrange(lo, hi + 1, step)
        if n not in forbidden and abs(n) not in forbidden:
            return n
    return hi - step


def split_ceiling(rng: random.Random, total: int, forbidden: set[int]) -> tuple[int, int]:
    for _ in range(200):
        rider = rng.randrange(60_000, min(280_000, total // 3), 25)
        base = total - rider
        if base > 0 and base not in forbidden and rider not in forbidden:
            return base, rider
    return total - 75_000, 75_000


def pick_names(rng: random.Random, n: int) -> list[tuple[str, str]]:
    used = set()
    out = []
    while len(out) < n:
        g = rng.choice(GIVEN)
        s = rng.choice(SURNAME)
        if (g, s) in used:
            s = s + rng.choice(["ton", "ford", "berg"])
        used.add((g, s))
        out.append((g, s))
    return out


def build_world(rng: random.Random, size: str) -> World:
    sc = SCALE[size]
    d = date(2024, rng.randint(6, 10), rng.randint(5, 24))
    award_date = d - timedelta(days=rng.randint(180, 230))
    visioning_date = d - timedelta(days=rng.randint(380, 470))
    kill_date = d - timedelta(days=rng.randint(70, 95))
    transfer_date = kill_date + timedelta(days=rng.randint(4, 10))
    precision_date = transfer_date + timedelta(days=rng.randint(6, 14))
    rebrand_date = d - timedelta(days=rng.randint(22, 40))
    pre_co_date = d - timedelta(days=rng.randint(8, 16))
    informal_date = d + timedelta(days=4)
    erratum_date = d + timedelta(days=9)
    gc_date = d + timedelta(days=15)
    name_change_date = d + timedelta(days=rng.randint(100, 130))
    directory_date = d + timedelta(days=rng.randint(190, 230))

    n_voting = sc["voting"]
    n_staff = sc["staff"]
    names = pick_names(rng, n_voting + n_staff)

    newest_appt = award_date - timedelta(days=rng.randint(25, 55))
    treas_appt = newest_appt - timedelta(days=rng.randint(50, 140))
    chair_appt = date(2016, rng.randint(3, 6), rng.randint(4, 20))
    other_span = (treas_appt - chair_appt).days - 30
    others = sorted(
        rng.sample(range(20, max(21, other_span)), n_voting - 3)
    )
    appts = [chair_appt] + [chair_appt + timedelta(days=x) for x in others]
    appts.append(treas_appt)
    appts.append(newest_appt)
    assert len(appts) == n_voting
    assert appts[0] == min(appts) and appts[-1] == max(appts)

    people: list[Person] = []
    board_roles = ["Chair", "Vice Chair"] + ["Trustee"] * (n_voting - 4) + ["Secretary", "Treasurer"]
    # Force: index 0 chair (earliest), -1 newest mill, -2 treasurer/certifying.
    board_roles[0] = "Chair"
    board_roles[-1] = "Trustee"
    board_roles[-2] = "Treasurer"
    board_roles[1] = "Vice Chair"
    board_roles[4 if n_voting > 4 else 2] = "Secretary"

    for i in range(n_voting):
        g, s = names[i]
        people.append(
            Person(
                pid=i,
                given=g,
                surname=s,
                role=board_roles[i],
                voting=True,
                appointed=appts[i],
                nickname=NICK.get(g),
                mill_district=(i == n_voting - 1),
            )
        )
    for j in range(n_staff):
        g, s = names[n_voting + j]
        people.append(
            Person(
                pid=n_voting + j,
                given=g,
                surname=s,
                role=STAFF_ROLES[j % len(STAFF_ROLES)],
                voting=False,
                appointed=date(2018, 1, 1) + timedelta(days=rng.randint(0, 2000)),
                nickname=NICK.get(g),
            )
        )

    chair = people[0]
    treasurer = people[n_voting - 2]
    newest = people[n_voting - 1]
    recused_member = people[2]
    absent_member = people[3]
    newest.mill_district = True
    treasurer.prior_surname = treasurer.surname
    extra_last = rng.choice([x for x in SURNAME if x != treasurer.surname])
    treasurer.surname = f"{treasurer.prior_surname}-{extra_last}"
    treasurer.name_change_date = name_change_date

    place = rng.choice(PLACES)
    vehicle = rng.choice(VEHICLES)
    phase = rng.choice(PHASES)
    cancelled = Project(
        pid=0,
        working_title=f"{place} {vehicle} {phase}",
        official_name=f"{place} Waterfront {vehicle} Service",
        rebrand_name=None,
        cancelled=True,
        absorbed_into=1,
        vendor=VENDORS[0],
        place=place,
        vehicle=vehicle,
        phase=phase,
    )
    route_n = rng.choice([12, 14, 16, 18, 21, 27])
    successor = Project(
        pid=1,
        working_title=f"Route {route_n} Evening Span Study",
        official_name=f"Route {route_n} Span Extension",
        rebrand_name=f"Riverline Evening Frequency Program",
        vendor=VENDORS[0],
    )
    rumor_project = Project(
        pid=2,
        working_title=f"Oakbridge Night-Owl Study",
        official_name="Oakbridge Night-Owl Study",
        rebrand_name=None,
    )
    sibling = Project(
        pid=3,
        working_title="Riverline Morning Frequency Program",
        official_name="Riverline Morning Frequency Program",
        rebrand_name=None,
    )
    projects = [cancelled, successor, rumor_project, sibling]
    used_titles = {p.working_title for p in projects} | {p.official_name for p in projects}
    pid = 4
    while len(projects) < sc["projects"]:
        p2 = rng.choice(PLACES)
        v2 = rng.choice(VEHICLES)
        ph = rng.choice(PHASES + ["Rehab", "Replacement", "Shelter Work", "Depot Roof"])
        title = f"{p2} {v2} {ph}"
        official = f"{p2} {ph} Capital"
        if title in used_titles or official in used_titles:
            continue
        used_titles.add(title)
        used_titles.add(official)
        projects.append(
            Project(
                pid=pid,
                working_title=title,
                official_name=official,
                rebrand_name=None,
                vendor=rng.choice(VENDORS),
            )
        )
        pid += 1

    recused_member.coi_projects = {cancelled.pid, successor.pid}
    recused_member.spouse_vendor = VENDORS[0]
    # Someone else had a COI on a distractor and divested — trap.
    decoy = people[5]
    decoy.coi_projects = {sibling.pid}
    decoy.spouse_vendor = VENDORS[2]
    decoy.coi_divest_date = d - timedelta(days=20)

    forbidden: set[int] = set()
    restated = unique_amount(rng, 1_175_000, 3_240_000, 25, forbidden)
    forbidden.add(restated)
    award_amt = unique_amount(rng, 800_000, restated - 150_000, 25, forbidden)
    forbidden.add(award_amt)
    pre_delta = unique_amount(rng, 180_000, 360_000, 25, forbidden)
    forbidden.add(pre_delta)
    forbidden.add(award_amt + pre_delta)

    later_specs: list[tuple[date, int, str, bool, date | None]] = []
    cursor = d + timedelta(days=22)
    n_cos = rng.randint(3, 4)
    void_index = rng.randint(0, n_cos - 2)
    net = restated
    for i in range(n_cos):
        delta = unique_amount(rng, 12_000, 92_000, 25, forbidden)
        if rng.random() < 0.45:
            delta = -delta
        forbidden.add(abs(delta))
        voided = i == void_index
        vdate = None
        label = rng.choice(
            [
                "asphalt-index rider",
                "signal-interconnect add",
                "shelter-lighting delta",
                "utility-relocation true-up",
                "spare-vehicle option",
                "weekend-span overtime rider",
            ]
        )
        later_specs.append((cursor, delta, label, voided, vdate))
        if voided:
            vdate = cursor + timedelta(days=rng.randint(12, 24))
            later_specs[-1] = (cursor, delta, label, True, vdate)
        else:
            net += delta
            forbidden.add(net)
        cursor = cursor + timedelta(days=rng.randint(18, 36))
    if net in (restated, award_amt, award_amt + pre_delta) or net <= 0:
        later_specs[-1] = (
            later_specs[-1][0],
            later_specs[-1][1] + 175,
            later_specs[-1][2],
            False,
            None,
        )
        net = restated + sum(s[1] for s in later_specs if not s[3])
    forbidden.add(net)

    ids = identity_set(cancelled, successor)
    motions: list[Motion] = []
    mid = 0

    def board_random_votes(
        meeting_date: date, proj: Project, pass_it: bool
    ) -> tuple[dict[int, str], set[int], set[int]]:
        sit = sitting(people, meeting_date)
        rec = {p.pid for p in sit if has_coi(p, {proj.pid} | (ids if proj.pid in ids else set()), meeting_date)}
        remaining = [p for p in sit if p.pid not in rec]
        votes: dict[int, str] = {}
        if pass_it:
            nays_n = max(1, len(remaining) // 4)
            nays = set(rng.sample([p.pid for p in remaining], nays_n))
            for p in remaining:
                votes[p.pid] = "nay" if p.pid in nays else "yea"
            yeas = sum(1 for v in votes.values() if v == "yea")
            nayc = sum(1 for v in votes.values() if v == "nay")
            if yeas <= nayc and remaining:
                votes[remaining[0].pid] = "yea"
        else:
            yeas_n = max(1, len(remaining) // 4)
            yea_set = set(rng.sample([p.pid for p in remaining], yeas_n))
            for p in remaining:
                votes[p.pid] = "yea" if p.pid in yea_set else "nay"
            yeas = sum(1 for v in votes.values() if v == "yea")
            nayc = sum(1 for v in votes.values() if v == "nay")
            if yeas > nayc and remaining:
                votes[remaining[-1].pid] = "nay"
        return votes, rec, set()

    award = Motion(
        mid=mid,
        meeting_date=award_date,
        project_id=successor.pid,
        kind="award",
        amount=award_amt,
        label="initial award",
        uses_name=successor.official_name,
    )
    tv, rec, ab = board_random_votes(award_date, successor, True)
    award.true_votes, award.recorded_votes, award.recused, award.absent = tv, dict(tv), rec, ab
    motions.append(award)
    mid += 1

    failed = Motion(
        mid=mid,
        meeting_date=rebrand_date - timedelta(days=5),
        project_id=successor.pid,
        kind="restatement",
        amount=unique_amount(rng, restated + 200_000, restated + 500_000, 25, forbidden),
        label="failed ceiling jump",
        uses_name=successor.official_name,
    )
    forbidden.add(failed.amount or 0)
    tv, rec, ab = board_random_votes(failed.meeting_date, successor, False)
    failed.true_votes, failed.recorded_votes, failed.recused, failed.absent = tv, dict(tv), rec, ab
    motions.append(failed)
    mid += 1

    pre_co = Motion(
        mid=mid,
        meeting_date=pre_co_date,
        project_id=successor.pid,
        kind="change_order",
        amount=pre_delta,
        label="pre-restatement fleet spare rider",
        uses_name=successor.rebrand_name or successor.official_name,
        true_votes={p.pid: "yea" for p in sitting(people, pre_co_date)},
        recorded_votes={},
    )
    pre_co.recorded_votes = dict(pre_co.true_votes)
    motions.append(pre_co)
    mid += 1

    restatement = Motion(
        mid=mid,
        meeting_date=d,
        project_id=successor.pid,
        kind="restatement",
        amount=restated,
        label="restated authorization",
        uses_name=successor.rebrand_name or successor.official_name,
    )
    sit = sitting(people, d)
    recused = {p.pid for p in sit if has_coi(p, ids, d)}
    recused.add(recused_member.pid)
    absent = {absent_member.pid}
    true_votes: dict[int, str] = {}
    forced_nay = {newest.pid, people[4].pid, people[min(5, n_voting - 1)].pid}
    for p in sit:
        if p.pid in recused:
            continue
        if p.pid in absent:
            continue
        true_votes[p.pid] = "nay" if p.pid in forced_nay else "yea"
    true_votes[treasurer.pid] = "yea"
    true_votes[chair.pid] = "yea"
    true_votes[newest.pid] = "nay"
    restatement.true_votes = true_votes
    recorded = dict(true_votes)
    recorded[newest.pid] = "yea"  # planted minutes error
    restatement.recorded_votes = recorded
    restatement.recused = recused
    restatement.absent = absent
    motions.append(restatement)
    mid += 1

    later_cos: list[Motion] = []
    for cdate, delta, label, voided, vdate in later_specs:
        co = Motion(
            mid=mid,
            meeting_date=cdate,
            project_id=successor.pid,
            kind="change_order",
            amount=delta,
            label=label,
            uses_name=successor.rebrand_name or successor.official_name,
            voided=voided,
            void_date=vdate,
            true_votes={p.pid: "yea" for p in sitting(people, cdate)[: max(5, n_voting - 2)]},
        )
        co.recorded_votes = dict(co.true_votes)
        motions.append(co)
        later_cos.append(co)
        mid += 1

    # Distractor motions with seductive amounts.
    for proj in (sibling, rumor_project, projects[min(6, len(projects) - 1)]):
        amt = unique_amount(rng, 900_000, 2_800_000, 50, forbidden)
        forbidden.add(amt)
        md = d + timedelta(days=rng.randint(-100, 80))
        m = Motion(
            mid=mid,
            meeting_date=md,
            project_id=proj.pid,
            kind="award",
            amount=amt,
            label="distractor award",
            uses_name=proj.rebrand_name or proj.official_name,
        )
        tv, rec, ab = board_random_votes(md, proj, True)
        m.true_votes, m.recorded_votes, m.recused, m.absent = tv, dict(tv), rec, ab
        motions.append(m)
        mid += 1

    assert motion_passed(restatement)
    cert = certifying_member(restatement, people)
    assert cert is not None and cert.pid == treasurer.pid, (
        cert.current_legal_name() if cert else None,
        treasurer.current_legal_name(),
    )
    computed_net = restated + sum(c.amount or 0 for c in later_cos if not c.voided)
    assert computed_net == net

    return World(
        rng_seed=0,
        size=size,
        people=people,
        projects=projects,
        motions=motions,
        restatement=restatement,
        cancelled=cancelled,
        successor=successor,
        net_amount=net,
        outcome="passed",
        certifying=cert,
        restatement_date=d,
        award_date=award_date,
        rebrand_date=rebrand_date,
        visioning_date=visioning_date,
        kill_date=kill_date,
        transfer_date=transfer_date,
        precision_date=precision_date,
        informal_date=informal_date,
        erratum_date=erratum_date,
        gc_date=gc_date,
        directory_date=directory_date,
        name_change_date=name_change_date,
        pre_co=pre_co,
        later_cos=later_cos,
        forbidden=forbidden,
        newest=newest,
        chair=chair,
        treasurer=treasurer,
        recused_member=recused_member,
        absent_member=absent_member,
        rumor_project=rumor_project,
        sibling=sibling,
    )


def addr(p: Person) -> str:
    return f"{p.legal_name_on(date(2020, 1, 1))} <{p.email_local()}>"


def staff_named(world: World, role_substr: str) -> Person:
    for p in world.people:
        if not p.voting and role_substr.lower() in p.role.lower():
            return p
    return [p for p in world.people if not p.voting][0]


def mention(p: Person, as_of: date, rng: random.Random, *, allow_role: bool = True) -> str:
    then = p.legal_name_on(as_of)
    last = then.split()[-1]
    opts = [then, f"{p.role} {last}"]
    if allow_role and p.role != "Trustee":
        opts.append(f"the {p.role}")
    if p.nickname:
        opts.append(f"{p.nickname} {last}")
    if p.mill_district:
        opts.append("the mill-district trustee")
        opts.append("the new trustee from the mill district")
    return rng.choice(opts)


def render_votes(m: Motion, world: World, rng: random.Random, recorded: bool) -> str:
    votes = m.recorded_votes if recorded else m.true_votes
    by_id = {p.pid: p for p in world.people}
    yeas, nays, abst = [], [], []
    for pid, v in votes.items():
        if pid in m.recused or pid in m.absent:
            continue
        phrase = mention(by_id[pid], m.meeting_date, rng)
        if v == "yea":
            yeas.append(phrase)
        elif v == "nay":
            nays.append(phrase)
        else:
            abst.append(phrase)
    rec_p = [mention(by_id[i], m.meeting_date, rng, allow_role=True) for i in sorted(m.recused)]
    abs_p = [mention(by_id[i], m.meeting_date, rng) for i in sorted(m.absent)]
    lines = []
    if yeas:
        if len(yeas) >= 2 and rng.random() < 0.7:
            lines.append(
                f"In the affirmative: {yeas[0]}; {yeas[1]}"
                + (f"; the latter joined remotely. Also: {'; '.join(yeas[2:])}." if len(yeas) > 2 else ".")
            )
        else:
            lines.append("In the affirmative: " + "; ".join(yeas) + ".")
    if nays:
        lines.append("In the negative: " + "; ".join(nays) + ".")
    if abst:
        lines.append("Abstaining: " + "; ".join(abst) + ".")
    if rec_p:
        lines.append(
            "Left the room for this item (conflict): " + "; ".join(rec_p) + "."
        )
    if abs_p:
        lines.append("Not present for the sitting: " + "; ".join(abs_p) + ".")
    return "\n".join(lines)


def email(frm: str, to: str, dt: date, subj: str, body: str) -> str:
    return (
        f"From: {frm}\nTo: {to}\nDate: {dt.isoformat()}\nSubject: {subj}\n\n{body.strip()}\n"
    )


def core_documents(rng: random.Random, world: World) -> list[str]:
    docs: list[str] = []
    w = world
    gm = staff_named(w, "General Manager")
    gc = staff_named(w, "Counsel")
    clerk = staff_named(w, "Recording")
    ops = staff_named(w, "Operations")
    fin = staff_named(w, "Finance")
    comms = staff_named(w, "Communications")
    c = w.cancelled
    s = w.successor
    board_to = "Board of Trustees <board@prtc.example>"

    docs.append(
        "PALISADE REGIONAL TRANSIT COOPERATIVE\n"
        "Public-records production — Board, Finance, Operations, Counsel\n"
        "Bates-unnumbered export. Documents are not in chronological order.\n"
        "Concatenated emails, minutes, directory extracts, and internal memos.\n"
    )

    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE — Bylaws (2019), reprint circulated "
        f"{fmt_date(rng, date(2023, 2, 14))}\n\n"
        "III.3 Conflicts. A member with a pecuniary conflict, including through a "
        "spouse, as to a procurement — or as to a cancelled procurement whose scope "
        "was transferred onto that procurement — shall recuse unless the conflict "
        "ended, by divestiture or otherwise, before the sitting.\n\n"
        "III.4 Presence. Only members physically or remotely present for the vote, "
        "and not recused, are eligible. Silence in the minutes is not presence if "
        "an erratum from the recording secretary later states the member had left.\n\n"
        "III.7 Audit certificate. Where a motion carries, the member among those "
        "eligible members voting in the affirmative who was most recently appointed "
        "to the Board shall sign the audit certificate. Recused and non-present "
        "members are ineligible. If two eligible affirmative voters share an "
        "appointment date, the lexicographically later legal surname then given "
        "name as they appear on the roster at the sitting signs. This section does "
        "not make the Chair the signatory by virtue of office.\n"
    )

    docs.append(
        email(
            addr(fin),
            board_to,
            date(2023, 11, 3),
            "Finance SOP 4.2 — restated authorizations",
            "Reminder, because this keeps coming up in capital ledgers:\n\n"
            "A restated authorization replaces the prior ceiling. Change orders "
            "adopted before that restatement are extinguished and must not be "
            "added back. Change orders adopted after the restatement adjust the "
            "new ceiling. A vacated (withdrawn) change order is treated as if it "
            "were never adopted. Voice-adopted riders still count if not later "
            "vacated. Informal running totals in staff chat are not the ledger.\n",
        )
    )

    docs.append(
        email(
            addr(ops),
            "planning@prtc.example",
            w.visioning_date,
            f"Visioning workshop notes — {w.visioning_date.year}",
            f"We parked three names on the whiteboard. Please do not treat these as "
            f"board-adopted titles.\n\n"
            f"1) {c.working_title} — community ask for restoring a waterfront loop "
            f"with independent procurement. Several people liked the romance of it.\n"
            f"2) {w.rumor_project.working_title} — separate, and not a waterfront item.\n"
            f"3) a vague 'evening span' doodle that planning already owns as "
            f"{s.official_name} in the capital calendar.\n\n"
            f"Also floated, then scratched: {c.place} {c.vehicle} Shelter {c.phase}, "
            f"which is a furniture project and must not be confused with (1).\n",
        )
    )

    civil, rider = split_ceiling(rng, w.restatement.amount or 0, w.forbidden)
    award_civil, award_rider = split_ceiling(rng, w.motions[0].amount or 0, w.forbidden)

    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE\nBoard of Trustees — Minutes\n"
        f"Sitting: {fmt_date(rng, w.award_date)} (regular)\n"
        f"Call to order by the Chair.\n\n"
        f"Motion on {s.official_name}: initial authorization. Staff presented a civil "
        f"package of {fmt_usd(award_civil)} and a systems rider of {fmt_usd(award_rider)} "
        f"as a combined ceiling, not as two contracts.\n"
        f"{render_votes(w.motions[0], w, rng, recorded=True)}\n"
        f"The chair stated that the motion carried.\n"
        f"Unrelated: snow-removal discussion; no procurement action.\nAdjourned.\n"
    )

    docs.append(
        email(
            addr(ops),
            board_to,
            w.kill_date,
            "Independent procurements we are not taking to bid",
            f"Quick status before finance closes Q2 language.\n\n"
            f"Regarding the {c.place} {c.vehicle.lower()} (the {c.phase.lower()} "
            f"proposal from visioning): it will not be bid independently. I do not "
            f"want another year of a zombie standalone.\n\n"
            f"This does not kill {w.rumor_project.official_name}, which some of you "
            f"keep bundling with the waterfront conversation. That rumor is wrong.\n",
        )
    )

    docs.append(
        email(
            addr(gc),
            addr(ops),
            w.transfer_date,
            "Re: independent-bid deaths and where the hours go",
            "Independent-bid items that died in Q2 have their service-hours "
            "transferred onto the span-extension work already in design. Do not "
            "open a second file. I will not bless two ceilings for one set of hours.\n",
        )
    )

    docs.append(
        email(
            addr(ops),
            board_to,
            w.precision_date,
            "Correction to my transfer note",
            f"To be precise, the only independent-bid death that transferred scope "
            f"was the waterfront {c.vehicle.lower()}, not {w.rumor_project.official_name} "
            f"and not the shelter-furniture cousin. Counsel's 'span-extension work' "
            f"means {s.official_name} as carried on the capital calendar.\n",
        )
    )

    docs.append(
        email(
            addr(comms),
            "allstaff@prtc.example",
            w.rebrand_date,
            "External name change — please stop using the route-number working title",
            f"Marketing has folded {s.official_name} into the Riverline family. "
            f"Effective immediately the program is to be called "
            f"{s.rebrand_name}. This is a label change. It is the same capital file. "
            f"It is not {w.sibling.official_name}, which remains a morning product "
            f"and a different authorization.\n",
        )
    )

    failed = next(m for m in w.motions if m.kind == "restatement" and m is not w.restatement)
    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE\nBoard of Trustees — Minutes\n"
        f"Sitting: {fmt_date(rng, failed.meeting_date)}\n"
        f"A motion to restate the ceiling for {failed.uses_name} at {fmt_usd(failed.amount or 0)} "
        f"was taken up and did not carry. Several members said the figure was a "
        f"wish, not a bid.\n{render_votes(failed, w, rng, recorded=True)}\n"
        f"No restatement occurred. Prior authorization remains in force.\n"
    )

    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE\nFinance Committee notes\n"
        f"{fmt_date(rng, w.pre_co.meeting_date)}\n"
        f"Without objection the board layered a {w.pre_co.label} of "
        f"{money_phrase(rng, w.pre_co.amount or 0)} onto the then-current "
        f"{w.pre_co.uses_name} ceiling. Staff remarked that this 'gets us close' "
        f"if no later restatement happens. That remark is not a restatement.\n"
    )

    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE\nBoard of Trustees — Minutes\n"
        f"Sitting: {fmt_date(rng, w.restatement_date)} (regular)\n"
        f"Recorder: {clerk.current_legal_name()}\n\n"
        f"Attendance was taken by office, not by legal name. The Chair presided. "
        f"{mention(w.treasurer, w.restatement_date, rng)} sat in the finance seat. "
        f"The mill-district seat was occupied.\n\n"
        f"Motion: restated authorization for {w.restatement.uses_name}. Staff said "
        f"the civil package is {fmt_usd(civil)} with a systems rider of {fmt_usd(rider)}, "
        f"taken together as the new ceiling. This restatement replaces prior "
        f"ceilings and riders. It is not an action on {w.sibling.official_name}.\n\n"
        f"The recording secretary listed the following expressions of position:\n"
        f"{render_votes(w.restatement, w, rng, recorded=True)}\n\n"
        f"The Chair announced that the motion had carried and asked that an audit "
        f"certificate be prepared 'the usual way'. No certificate was signed in the room.\n"
        f"Adjourned.\n"
    )

    docs.append(
        email(
            addr(fin),
            addr(clerk),
            w.informal_date,
            "audit certificate — just have the Chair sign",
            f"For the audit certificate on last week's restatement, just have the "
            f"Chair sign — that's the certifying member in practice. "
            f"{w.chair.legal_name_on(w.restatement_date)} always does it, and nobody "
            f"on the ops side wants to read III.7. Please don't make this a production.\n",
        )
    )

    docs.append(
        email(
            addr(clerk),
            board_to,
            w.erratum_date,
            f"Erratum — minutes of {w.restatement_date.isoformat()}",
            f"The draft minutes incorrectly recorded {mention(w.newest, w.restatement_date, rng)} "
            f"as being in the affirmative on the restated authorization for "
            f"{w.restatement.uses_name}. The member has written, and I concur from "
            f"the tape, that the vote was in the negative. The mill-district trustee "
            f"did not recuse; they simply opposed. Please replace the roll accordingly. "
            f"This erratum does not touch other items from that sitting.\n",
        )
    )

    docs.append(
        email(
            addr(gc),
            board_to,
            w.gc_date,
            "Bylaw III.7 still controls — ignore the 'Chair signs' explainer",
            "Several staff have described the Chair as the certifying member. That "
            "is incorrect. Bylaw III.7 is unchanged since 2019: the audit certificate "
            "is signed by the eligible member who voted Yea and whose appointment "
            "date is the most recent among those Yea votes (ties: legal surname then "
            "given name as of the sitting). Recusals and persons not present are "
            "ineligible. The minutes are evidence but yield to written errata from "
            "the recording secretary. The 2019 text controls over later informal "
            "explainers, including finance's note to the clerk after the September "
            "sitting. Do not invent a custom of Chair-certification.\n",
        )
    )

    docs.append(
        email(
            addr(w.recused_member),
            addr(gc),
            w.award_date + timedelta(days=12),
            "Annual conflict disclosure",
            f"I am disclosing that my spouse is a project engineer at {VENDORS[0]}, "
            f"which is expected to bid the span-extension work and had also been "
            f"discussed for the waterfront {c.vehicle.lower()} hours. I will leave "
            f"the room for those items until I tell you otherwise. I have not "
            f"divested anything.\n",
        )
    )

    docs.append(
        email(
            addr(w.people[5]),
            addr(gc),
            w.restatement_date - timedelta(days=21),
            "Divestiture — sibling product only",
            f"I sold the {VENDORS[2]} shares last week. That was the only holding "
            f"tied to {w.sibling.official_name}. It has nothing to do with the "
            f"evening Riverline file. Please lift that recusal.\n",
        )
    )

    for co in w.later_cos:
        docs.append(
            f"PALISADE REGIONAL TRANSIT COOPERATIVE — excerpt, {fmt_date(rng, co.meeting_date)}\n"
            f"The board, without objection, adopted a change to {co.uses_name} "
            f"described as the {co.label}: {money_phrase(rng, co.amount or 0)} relative "
            f"to the restated ceiling. This is not a new restatement.\n"
        )
        if co.voided and co.void_date:
            docs.append(
                email(
                    addr(fin),
                    board_to,
                    co.void_date,
                    f"Withdrawal of the {co.label}",
                    f"The {co.label} adopted on {co.meeting_date.isoformat()} for "
                    f"{co.uses_name} is withdrawn and vacated. Do not include that "
                    f"delta in the authorization ledger. The restated ceiling is not "
                    f"itself reopened by this withdrawal.\n",
                )
            )

    docs.append(
        email(
            f"{w.treasurer.given} {w.treasurer.surname} <{w.treasurer.email_local()}>",
            "directory@prtc.example",
            w.name_change_date,
            "Legal name update — roster",
            f"Please update the certified roster. {w.treasurer.given} "
            f"{w.treasurer.prior_surname} is now {w.treasurer.current_legal_name()}. "
            f"Same person, same finance seat, same appointment date of "
            f"{w.treasurer.appointed.isoformat()}. Mail to the old local-part still "
            f"reaches me. This is not a new appointment and is not a vacancy.\n",
        )
    )

    # Appointments scattered, using then-current names, no end-of-record surnames
    # for the treasurer.
    for p in [x for x in w.people if x.voting]:
        then = p.legal_name_on(p.appointed)
        who = "mill-district seat" if p.mill_district else p.role
        docs.append(
            email(
                addr(gm),
                "allstaff@prtc.example",
                p.appointed,
                f"Board appointment — {who}",
                f"Effective {fmt_date(rng, p.appointed)}, {then} takes the {who}. "
                f"Please update the dais cards. This note is not a vote tally.\n",
            )
        )

    dir_lines = [
        f"PRTC CERTIFIED ROSTER — close of record {w.directory_date.isoformat()}",
        "Legal names below are current. Appointment dates are intentionally omitted "
        "here; they live in the historical appointment mail.",
        "",
    ]
    for p in w.people:
        tag = "voting trustee" if p.voting else "staff"
        dir_lines.append(f"- {p.current_legal_name()}, {p.role} ({tag})")
        if p.nickname:
            dir_lines.append(f"  informal: {p.nickname}")
    docs.append("\n".join(dir_lines) + "\n")

    # Snow-contract trap using the Chair as "certifying member".
    snow_amt = unique_amount(rng, 40_000, 90_000, 25, w.forbidden)
    docs.append(
        email(
            addr(fin),
            board_to,
            w.restatement_date + timedelta(days=3),
            "snow-removal audit certificate",
            f"The winter service contract at {fmt_usd(snow_amt)} is unrelated to "
            f"Riverline. The Chair signed that audit certificate. Please do not "
            f"generalize that custom to capital restatements.\n",
        )
    )

    # Sibling minutes with a plump dollar amount.
    sib = next(m for m in w.motions if m.project_id == w.sibling.pid)
    docs.append(
        f"PALISADE REGIONAL TRANSIT COOPERATIVE — Minutes {fmt_date(rng, sib.meeting_date)}\n"
        f"Authorization for {w.sibling.official_name} at a ceiling of "
        f"{fmt_usd(sib.amount or 0)}. This morning product is not the evening file "
        f"and does not absorb waterfront hours.\n"
        f"{render_votes(sib, w, rng, recorded=True)}\n"
    )
    return docs


def extra_minutes(rng: random.Random, world: World, n: int) -> list[str]:
    docs = []
    start = date(2023, 2, 1)
    span = (world.directory_date - start).days
    for _ in range(n):
        dt = start + timedelta(days=rng.randint(0, span))
        p = rng.choice(world.projects[3:])
        amt = unique_amount(rng, 15_000, 420_000, 25, world.forbidden)
        topic = rng.choice(
            [
                "bus-stop snow rotation",
                "union impact bargaining update",
                "paratransit no-show policy",
                "lost-and-found protocol",
                "radio-system patch window",
                "operator restroom agreement",
                "farebox firmware",
                "bridge-clearance detour",
            ]
        )
        speaker = rng.choice([x for x in world.people if x.voting])
        docs.append(
            f"PRTC — {rng.choice(['Operations huddle', 'Committee of the whole', 'Work session'])}\n"
            f"{fmt_date(rng, dt)}\n"
            f"{mention(speaker, dt, rng)} reported on {topic} touching {p.official_name}. "
            f"A placeholder figure of {fmt_usd(amt)} was sketched and explicitly marked "
            f"non-binding. No restatement of any Riverline file occurred. "
            f"Someone asked whether Bylaw III.7 applied; counsel said there was no "
            f"carried motion and therefore no certificate.\n"
        )
    return docs


def ridership_report(rng: random.Random, world: World, min_chars: int) -> str:
    year = rng.choice([2023, 2024, 2025])
    month = rng.randint(1, 12)
    routes = [f"R{n}" for n in range(1, 32)]
    lines = [
        f"PRTC RIDERSHIP EXTRACT — {year:04d}-{month:02d}",
        "Unaudited APC counts. Not a procurement record. Not a vote.",
        f"Prepared for {staff_named(world, 'Operations').current_legal_name()}.",
        "",
        "route,stop,day,ons,offs,note",
    ]
    notes = [
        "ok",
        "sensor-glitch",
        "football special",
        "weather",
        "detour",
        "funeral crowd",
        "shift-change spike",
        "",
    ]
    # Grow until we clear min_chars.
    day = 1
    while sum(len(x) + 1 for x in lines) < min_chars:
        rt = rng.choice(routes)
        stop = f"{rng.choice(STREETS)} {rng.choice(['& 3rd', 'Terminal', 'Park', 'School', 'Yard'])}"
        ons = rng.randint(0, 220)
        offs = rng.randint(0, 220)
        lines.append(
            f"{rt},{stop},{year:04d}-{month:02d}-{day:02d},{ons},{offs},{rng.choice(notes)}"
        )
        day = 1 + (day % 28)
        if len(lines) > min_chars:  # safety
            break
    return "\n".join(lines) + "\n"


def chatter(rng: random.Random, world: World) -> str:
    a, b = rng.sample(world.people, 2)
    dt = date(2023, 1, 15) + timedelta(days=rng.randint(0, 900))
    topic = rng.choice(
        [
            "the copier on floor two is jammed again",
            "please stop parking in the operator slots",
            "cake in the break room for the safety milestone",
            "the website still says we take checks on Sunday",
            "lost backpack at the foundry stop, purple strap",
            "who has the key to the salt shed",
            "the new uniforms shrink if you actually wash them",
            "can we not reply-all to the bridge detour thread",
        ]
    )
    return email(
        addr(a),
        addr(b),
        dt,
        rng.choice(["fyi", "quick question", "not urgent", "re: facilities"]),
        f"{topic}. cc'ing nobody else. This has no effect on capital authorizations "
        f"and is not about {world.sibling.official_name} either.\n"
        f"Also {rng.choice(STREETS)} Street shelter glass is cracked; {rng.choice(VENDORS)} "
        f"might quote it. Placeholder only: {fmt_usd(unique_amount(rng, 800, 9000, 25, world.forbidden))}.\n",
    )


def render_docs(rng: random.Random, world: World, target: int) -> list[str]:
    docs = core_documents(rng, world)
    docs.extend(extra_minutes(rng, world, SCALE[world.size]["extra_meetings"]))
    total = sum(len(d) + 2 for d in docs)
    reports = SCALE[world.size]["reports"]
    for i in range(reports):
        need = 4_000 if world.size == "small" else (12_000 if world.size == "medium" else 18_000)
        if total >= target and i > 0:
            break
        r = ridership_report(rng, world, need)
        docs.append(r)
        total += len(r) + 2
    guard = 0
    while total < target and guard < 20_000:
        guard += 1
        need = min(max(target - total, 350), 6_000)
        if rng.random() < 0.55:
            f = chatter(rng, world)
            if len(f) < need:
                f = f + ridership_report(rng, world, need - len(f) + 50)
        else:
            f = ridership_report(rng, world, need)
        docs.append(f)
        total += len(f) + 2
    rng.shuffle(docs)
    return docs


def generate(seed: int, size: str) -> dict[str, Any]:
    if size not in TARGET_CHARS:
        raise ValueError("size must be one of small, medium, large")
    rng = random.Random(seed)
    world = build_world(rng, size)
    world.rng_seed = seed
    docs = render_docs(rng, world, TARGET_CHARS[size])
    context = "\n\n".join(docs)
    question = (
        "From the Palisade Regional Transit Cooperative public-records dump, "
        "determine the net authorized contract amount (integer USD) for the "
        "project that is the current legal successor of the initiative first "
        f"raised under the working title {world.cancelled.working_title!r}, "
        "after applying the Finance SOP restatement rule, every later change "
        "order, and every voidance. Using Bylaw III.7 — the General Counsel's "
        "correction of the informal explainer controls — identify the current "
        "legal name of the certifying member for the last successful restated "
        "authorization of that successor. Also state whether that restated "
        "authorization passed. Ignore informal commentary that contradicts the "
        "bylaws or the SOP."
    )
    current = world.certifying.current_legal_name()
    variants = [current]
    if world.certifying.nickname:
        variants.append(f"{world.certifying.nickname} {world.certifying.surname}")
    answer = {
        "outcome": world.outcome,
        "net_amount": world.net_amount,
        "certifying_member": current,
        "name_variants": variants,
    }
    meta = {
        "seed": seed,
        "size": size,
        "context_chars": len(context),
        "n_documents": len(docs),
        "working_title": world.cancelled.working_title,
        "official_name": world.successor.official_name,
        "rebrand_name": world.successor.rebrand_name,
        "restatement_date": world.restatement_date.isoformat(),
        "restated_ceiling": world.restatement.amount,
        "net_amount": world.net_amount,
        "certifying_member": current,
        "certifying_name_at_vote": world.certifying.legal_name_on(world.restatement_date),
        "planted_false_certifier_chair": world.chair.current_legal_name(),
        "planted_minutes_error_member": world.newest.current_legal_name(),
    }
    return {"context": context, "question": question, "answer": answer, "meta": meta}


def _extract_ints(text: str) -> set[int]:
    found: set[int] = set()
    for m in re.finditer(r"-?\$?\d[\d,]*", text):
        raw = m.group().replace("$", "").replace(",", "")
        try:
            found.add(int(raw))
        except ValueError:
            continue
    return found


def _name_hit(text: str, truth: dict[str, Any]) -> bool:
    t = _norm(text)
    for v in truth.get("name_variants") or [truth["certifying_member"]]:
        nv = _norm(v)
        if nv and nv in t:
            return True
    parts = _norm(truth["certifying_member"]).split()
    if len(parts) >= 2:
        tokens = set(t.split())
        if parts[0] in tokens and parts[-1] in tokens:
            return True
    return False


def _outcome_hit(text: str, truth: dict[str, Any]) -> bool:
    t = _norm(text)
    if truth["outcome"] == "passed":
        if re.search(r"\b(failed|rejected|defeated)\b", t) and not re.search(
            r"\b(passed|carried|approved|adopted)\b", t
        ):
            return False
        return bool(re.search(r"\b(passed|carried|approved|adopted)\b", t))
    return bool(
        re.search(
            r"\b(failed|rejected|defeated|did not (?:pass|carry)|not (?:passed|approved|adopted))\b",
            t,
        )
    )


def score(answer_text: str, truth: Any) -> float:
    if not isinstance(truth, dict) or not isinstance(answer_text, str):
        return 0.0
    tenths = 0
    if truth.get("net_amount") in _extract_ints(answer_text):
        tenths += 4
    if _name_hit(answer_text, truth):
        tenths += 4
    if _outcome_hit(answer_text, truth):
        tenths += 2
    return tenths / 10.0


if __name__ == "__main__":
    for size in ("small", "medium", "large"):
        g = generate(0, size)
        print(f"size={size} chars={len(g['context'])} docs={g['meta']['n_documents']}")
        print(f"question={g['question']}")
        print(f"truth={g['answer']}")
        truth = g["answer"]
        assert score(str(truth), truth) == 1.0
        assert score("", truth) < 1.0
        assert score("nope", truth) < 1.0
        assert score("The motion passed. Certifying member: Santa Claus. Amount: $12.", truth) < 1.0
        almost = (
            f"The restated authorization passed. Certifying member: "
            f"{truth['certifying_member']}. Net amount: $12."
        )
        assert score(almost, truth) < 1.0
        print("asserts ok")
