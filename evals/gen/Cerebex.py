#!/usr/bin/env python3.11
"""
Long-context evaluation task: "Amended Travel Reimbursement Audit".

The context is a chronological corporate email archive. Buried in it are expense
reports described in prose; many are later amended by follow-up emails (amount
corrections, approvals rescinded, denials reversed, approvers delegated).

QUESTION: total USD of *travel* reports whose FINAL status (after every
correction) is "approved", and whose effective approver is a manager in the
Operations division (accounting for delegation emails).

WHY GREP/REGEX FAILS:
 - The tokens "travel", "approved", "$<number>" occur pervasively in distractor
   emails (other categories, other divisions, negations like "we will not be
   approving", chatter about travel bookings that are not expense reports).
 - Amendments never cite report IDs in most cases; they refer to reports by
   paraphrase ("the Oslo invoice Maria sent in March", "scratch that figure I
   gave you"). The final amount of a report is often stated ONLY as a
   correction of an earlier amount ("the real total came to $X, not $Y").
 - Approval emails sometimes reverse an earlier denial, and rescission emails
   reverse an earlier approval, so surface status words are unreliable.
 - Two employees can share a first name across divisions; coreference
   ("her manager", "the latter") must be resolved.
The answer therefore requires reading/reasoning over the whole document.
"""

import random
import re

FIRST = ["Ana","Ben","Carla","Dmitri","Elena","Farid","Grace","Hugo","Imani",
         "Jonas","Kira","Liam","Mara","Nadia","Omar","Priya","Quinn","Rosa",
         "Sam","Tomas","Uma","Viktor","Wen","Yara","Zane","Leo","Nina","Paul"]
LAST  = ["Okafor","Lindqvist","Marino","Chen","Duval","Haddad","Novak","Silva",
         "Kowalski","Brennan","Ito","Petrov","Alvarez","Mbeki","Fischer",
         "Rossi","Larsen","Cruz","Devi","Weber"]
CITIES = ["Oslo","Lisbon","Kyoto","Austin","Marseille","Gdansk","Nairobi",
          "Bogota","Hanoi","Perth","Lyon","Riga","Denver","Seville","Tampere"]
CATEGORIES = ["travel","meals","office supplies","software licences","training"]
DIVISIONS  = ["Operations","Engineering","Sales","Marketing","Finance"]
MONTHS = ["January","February","March","April","May","June","July","August",
          "September","October","November","December"]
PURPOSE = ["the client kickoff","the vendor audit","regional onboarding",
           "the site inspection","the partner summit","training delivery",
           "the quarterly review","field support"]
_MONTH_POSSESSIVE = 'this month’s'

MONEY = lambda rng: rng.choice([240,380,615,840,1120,1470,1890,2260,2740,
                                3180,3950,4600]) + rng.randrange(0,90)

def _usd(x):
    return "${:,.2f}".format(x)

def _email(rng, day, frm, to, subj, body):
    return (f"From: {frm}\nTo: {to}\nDate: {MONTHS[day%12]} "
            f"{(day//12)%28+1}, 2024\nSubject: {subj}\n\n{body}\n")

def _filler(rng, day, people, mgrs):
    tpl = rng.randrange(6)
    a, b = rng.sample(people, 2)
    if tpl == 0:
        return _email(rng, day, a, b, "Re: parking situation",
            "The lower deck is closed for resurfacing this week. Please use the "
            "visitors' lot and badge in at the side entrance.")
    if tpl == 1:
        return _email(rng, day, a, b, "Standup notes",
            "Short one today: the deployment is frozen until Thursday, QA owns "
            "the regression list, and we skip the demo. Nothing else pending.")
    if tpl == 2:
        m = rng.choice(mgrs)
        return _email(rng, day, a, m, "Team lunch?",
            f"Can we book something near the office for {rng.choice(MONTHS)}? "
            "Nothing formal, just the usual group.")
    if tpl == 3:
        return _email(rng, day, a, b, "Printer / badge / misc",
            "Reminder to return visitor badges at reception, and the 3rd floor "
            "printer is out of toner again. Facilities has been notified.")
    if tpl == 4:
        c = rng.choice(CITIES)
        return _email(rng, day, a, b, f"Flights to {c}",
            f"I am looking at fares to {c} for the team offsite but we have NOT "
            "submitted any expense report yet — this is just research.")
    return _email(rng, day, a, b, "Merged: agenda",
        "Carrying over the open items from last cycle; nothing decided today, "
        "we will revisit after the numbers come back from finance.")

def generate(seed: int, size: str) -> dict:
    rng = random.Random(seed)
    target = {"small": 60_000, "medium": 300_000, "large": 1_200_000}[size]
    n_reports = {"small": 14, "medium": 55, "large": 230}[size]

    # ---- organisations: divisions -> managers -> employees ----------------
    used = set()
    def name():
        while True:
            n = f"{rng.choice(FIRST)} {rng.choice(LAST)}"
            if n not in used:
                used.add(n); return n
    mgrs, emp_of, div_of, man_of = [], {}, {}, {}
    for d in DIVISIONS:
        for _ in range(rng.choice([1, 2])):
            m = name(); mgrs.append(m); div_of[m] = d
    employees = []
    for _ in range(n_reports + 8):
        e = name()
        m = rng.choice(mgrs)
        emp_of[e] = m; man_of[e] = m; div_of[e] = div_of[m]
        employees.append(e)
    people = employees + mgrs
    # deliberate trap: one employee shares a first name with another person
    if len(employees) > 4:
        twin = employees[0].split()[0]
        dup = f"{twin} {rng.choice(LAST)}"
        if dup not in used:
            used.add(dup)
            m2 = rng.choice(mgrs)
            while div_of[m2] == div_of[employees[0]]:
                m2 = rng.choice(mgrs)
            emp_of[dup] = m2; man_of[dup] = m2; div_of[dup] = div_of[m2]
            employees.append(dup); people.append(dup)

    # delegation: one Operations manager delegates approvals while away.
    # The delegate is drawn from the same Operations division so the
    # question's "effective approver belongs to Operations" clause is
    # satisfied for delegated reports (and the text and the key agree).
    ops_mgrs = [m for m in mgrs if div_of[m] == "Operations"]
    deputy = None
    if ops_mgrs:
        delegator = ops_mgrs[0]
        ops_emps = [e for e in employees if div_of[e] == "Operations"
                    and emp_of[e] != delegator]
        if ops_emps:
            deputy = rng.choice(ops_emps)
        else:
            # Fallback: the other Operations manager (if any) deputies.
            others = [m for m in ops_mgrs if m != delegator]
            if others:
                deputy = rng.choice(others)
    reports = []
    day = rng.randrange(40, 90)
    for i in range(n_reports):
        e = rng.choice(employees)
        cat = rng.choice(CATEGORIES)
        amt = MONEY(rng)
        city = rng.choice(CITIES)
        code = f"EX-{rng.randrange(1000, 9999)}" if rng.random() < 0.35 else None
        approver = deputy if (deputy and emp_of[e] == ops_mgrs[0]) else man_of[e]
        approved = rng.random() < 0.62
        reports.append(dict(emp=e, cat=cat, amt=amt, city=city, code=code,
                            approver=approver, approved=approved,
                            day=day, touched=False))
        day += rng.randrange(2, 9)

    # Guarantee the truth is non-zero on most seeds: if no Operations-approved
    # travel report exists, flip the FIRST travel report from an Operations
    # employee to approved (or pick any report and turn it into a travel,
    # Operations-approved report). This only runs when nothing qualifies
    # so the natural "answer is 0" cases are preserved.
    ops_travel_approved = [r for r in reports
                           if r["cat"] == "travel" and r["approved"]
                           and div_of[r["approver"]] == "Operations"]
    if not ops_travel_approved:
        # Try to flip an existing travel report whose approver is Operations.
        candidates = [r for r in reports
                      if r["cat"] == "travel"
                      and div_of[r["approver"]] == "Operations"]
        if candidates:
            candidates[0]["approved"] = True
        else:
            # No Ops travel at all: convert the first non-travel report from an
            # Operations employee into a travel report that is approved.
            candidates = [r for r in reports
                          if div_of[r["approver"]] == "Operations"]
            if candidates:
                c = candidates[0]
                c["cat"] = "travel"
                c["approved"] = True

    # ---- emit the archive --------------------------------------------------
    parts, day = [], 0

    def emit(txt):
        parts.append(txt)

    # opening chatter so the archive does not start with the answer material
    for _ in range(6):
        emit(_filler(rng, day, people, mgrs)); day += 1

    for r in reports:
        # 1) submission
        catdesc = (f"{r['city']} trip for {rng.choice(PURPOSE)}"
                   if r["cat"] == "travel" else
                   f"{r['cat']} purchases ({rng.choice(['the', _MONTH_POSSESSIVE, 'routine'])} batch)")
        body = (f"Hi {r['approver'].split()[0]},\n\nPlease find my reimbursement "
                f"request for {catdesc}. The total came to {_usd(r['amt'])}, "
                "receipts attached."
                + (f" Reference {r['code']}." if r['code'] else ""))
        emit(_email(rng, r["day"], r["emp"], r["approver"],
                    f"Reimbursement request - {catdesc}", body))
        # 2) approval / denial (sometimes a negation-heavy denial)
        r["day"] += rng.randrange(1, 5)
        if r["approved"]:
            emit(_email(rng, r["day"], r["approver"], r["emp"],
                        f"Re: Reimbursement request - {catdesc}",
                 f"Approved at {_usd(r['amt'])}. Please cc finance."))
        else:
            emit(_email(rng, r["day"], r["approver"], r["emp"],
                        f"Re: Reimbursement request - {catdesc}",
                 rng.choice([f"We will not be approving this one — the "
                             f"{_usd(r['amt'])} figure is out of policy.",
                             "Not approved this cycle; no exceptions.",
                             "I cannot sign off on this request."])))
        # 3) amendment (~35%): amount correction, status flip, or both
        if rng.random() < 0.35:
            r["touched"] = True
            r["day"] += rng.randrange(2, 10)
            kind = rng.randrange(3)
            ref = (f"the {r['city']} invoice" if r["cat"] == "travel"
                   else f"that {r['cat']} batch")
            who = rng.choice([r["emp"], r["approver"]])
            if kind == 0:      # amount correction overrides old figure
                new = MONEY(rng)
                body = (f"Correction on {ref} from {r['emp'].split()[0]}: the "
                        f"real total came to {_usd(new)}, not "
                        f"{_usd(r['amt'])}. Please use the newer figure and "
                        "disregard my earlier email.")
                r["amt"] = new
            elif kind == 1:    # status flip
                r["approved"] = not r["approved"]
                body = rng.choice([
                    f"Scratch that — after reviewing {ref}, we have decided to "
                    "approve it after all at the amount originally submitted.",
                    f"Update on {ref}: we are rescinding the approval; it does "
                    "not qualify, despite what I wrote previously."])
            else:              # both
                new = MONEY(rng)
                r["amt"] = new; r["approved"] = rng.random() < 0.5
                body = (f"Two changes on {ref}: final amount is {_usd(new)} "
                        f"(replacing the earlier number), and the decision is "
                        f"now {'approved' if r['approved'] else 'declined'}.")
            emit(_email(rng, r["day"], who, rng.choice(people),
                        f"Re: Reimbursement request - {catdesc}", body))

    # coreference traps: "approve the latter of the two"
    for _ in range(max(2, n_reports // 6)):
        a, b = rng.sample(reports, 2)
        if div_of[a["approver"]] == "Operations" and a["cat"] == "travel":
            chosen = a
        elif div_of[b["approver"]] == "Operations" and b["cat"] == "travel":
            chosen = b
        else:
            chosen = rng.choice([a, b])
        chosen["approved"] = True; chosen["touched"] = True
        chosen["day"] += 1
        emit(_email(rng, chosen["day"], chosen["approver"], chosen["emp"],
                    "Decision on the two outstanding requests",
             f"Between {a['emp'].split()[0]}'s {a['city']} paperwork and "
             f"{b['emp'].split()[0]}'s {b['city']} paperwork, approve the "
             f"{'latter' if chosen is b else 'former'} at the submitted amount."))

    # delegation email (positioned early: solver must remember it globally)
    if deputy:
        emit(_email(rng, rng.randrange(5, 20), ops_mgrs[0], deputy,
                    "Approvals while I am on leave",
             f"While I am away all {div_of[ops_mgrs[0]]} expense approvals go "
             f"through {deputy.split()[0]} — sign in my name and cc me."))
    if len(employees) > 4:
        emit(_email(rng, rng.randrange(20, 40), mgrs[0], employees[-1],
                    "Names — heads up",
             f"To avoid confusion: {employees[-1]} and {employees[0]} share a "
             "first name but sit in different reporting lines. Address them by "
             "full name in anything official."))

    # DEFECT FIX: a staff-directory email so the user can resolve which
    # approvers are in the Operations division. The original task hid the
    # division mapping entirely, leaving "0" as the only safe answer for any
    # seed where the original manager's reports get redirected to a
    # non-Operations deputy. Listing every manager + their division makes the
    # question's "effective approver belongs to the Operations division"
    # clause actually answerable.
    dir_lines = [f"  - {m} ({div_of[m]})" for m in sorted(mgrs, key=lambda x: div_of[x] + x)]
    emit(_email(rng, rng.randrange(5, 20), "HR Operations", "All",
                "Staff directory — current reporting lines",
                "For everyone's reference, here is the current reporting "
                "structure by division. Use this when an email refers to "
                "an approver by name and you need to confirm their division.\n\n"
                + "Managers:\n" + "\n".join(dir_lines)))

    # filler to reach the exact character target
    while sum(len(p) for p in parts) < target:
        emit(_filler(rng, day, people, mgrs)); day += 1
    context = "\n\n".join(parts)

    # ---- ground truth from the internal structures -------------------------
    truth = sum(r["amt"] for r in reports
                if r["cat"] == "travel" and r["approved"]
                and div_of[r["approver"]] == "Operations")

    question = ("The archive below contains expense reimbursement emails. Some "
                "requests were later amended by follow-up messages; where a "
                "message corrects or reverses an earlier one, the correction "
                "governs. What is the total dollar amount, in USD, of the "
                "travel-category reimbursements whose final status (after all "
                "amendments, reversals and delegation rules) is 'approved' AND "
                "whose effective approver belongs to the Operations division "
                "(the approver being the employee's manager, or the delegate "
                "named in any delegation notice)? Reply with just the number.")
    meta = {"n_reports": n_reports, "n_chars": len(context),
            "n_emails": len(parts), "delegation": bool(deputy),
            "size": size, "seed": seed}
    return {"context": context, "question": question,
            "answer": truth, "meta": meta}

_NUM = re.compile(r"-?\d[\d,]*(?:\.\d+)?")
_NEG_BEFORE = re.compile(
    r"\b(?:not|never|wrong(?:ly)?|incorrect(?:ly)?|no(?!\s+\w*qualif)|"
    r"earlier|previously|originally|at\s+first|first|initially)\b",
    re.IGNORECASE,
)
# A "stated total" hint phrase is what most users use to declare the answer;
# the number that follows the LAST such phrase is the authoritative figure.
# $ and "Total:" / "total =" are included so leading-claim forms like
# "$910 across the qualifying claims (3 claims)" or
# "Qualifying: 300 + 610 = 910. Total: 910" still resolve to the right number.
_TOTAL_HINT = re.compile(
    r"(?:"
    r"\bfinal\s+total|\bthe\s+total|\btotal\s+is|\btotal\s+comes\s+to|"
    r"\btotal\s*[:=]|"
    r"\banswer\s+is|\banswer\s*[:=]|\bfinal\s+answer|\bconclusion\s+is|"
    r"\bqualifying\s+total|\breconciled\s+total|\bapproved\s+total|"
    r"\bso\s+the\s+total|\bso\s+the\s+answer|"
    r"\bso\s+the\s+net|\bfinal\s+number|"
    r"\bin\s+total|\baltogether|"
    r"\bmy\s+answer|\bmy\s+total|"
    r"\bi\s+got|\bi\s+conclude|\bconclusion|"
    r"\$\s*"
    r")",
    re.IGNORECASE,
)
_ZERO_WORDS = {
    "zero": 0, "none": 0, "nothing": 0, "noone": 0, "nada": 0,
    "nil": 0, "no": 0,
}


def _parse(tok: str):
    try:
        return float(tok.replace(",", ""))
    except ValueError:
        return None


def _number_values(text: str):
    """Yield (value, start, end) for every numeric token in text."""
    for m in _NUM.finditer(text):
        v = _parse(m.group(0))
        if v is not None:
            yield v, m.start(), m.end()


def _zero_word_value(text: str):
    """If the answer uses 'zero' or 'none' to mean 0, return (0, position).

    Uses word boundaries so that "no" inside "not" or "north" does not
    accidentally trigger a 0-detection.
    """
    text_low = text.lower()
    for w in _ZERO_WORDS:
        m = re.search(r"\b" + re.escape(w) + r"\b", text_low)
        if m:
            return _ZERO_WORDS[w], m.start()
    return None


def score(answer_text: str, truth) -> float:
    """Robust to prose/formatting; STRICT about the substance.

    Authoritative-figure selection (in order):
    1. The number following the LAST "stated total" hint phrase
       ("the total is", "final total", "answer:", etc.).
    2. Otherwise the LAST numeric token NOT in a negation context
       ("not X", "(not X)", "earlier I got X").

    Full credit (1.0) only if the authoritative figure equals the truth
    (within a cent). 0.3 if the truth appears somewhere in the answer but
    the authoritative figure is different. 0.0 otherwise. "None qualify"
    / "zero" phrasings are treated as 0 for the purpose of this match.
    """
    try:
        t = float(truth)
    except (TypeError, ValueError):
        return 0.0
    if not answer_text or not str(answer_text).strip():
        return 0.0
    text = str(answer_text)

    # Find the position of the last "stated total" hint, if any.
    last_hint_end = -1
    for m in _TOTAL_HINT.finditer(text):
        last_hint_end = max(last_hint_end, m.end())
    # Find the first number AFTER the last hint, or fall back to the last
    # number in the text.
    numbers = list(_number_values(text))
    if not numbers:
        # Check for zero-word only.
        zw = _zero_word_value(text)
        if zw is not None:
            return 1.0 if abs(t - 0) < 0.005 else 0.0
        return 0.0
    # Authoritative = first number after the last hint (if a hint exists),
    # else the last number in the text.
    if last_hint_end >= 0:
        after_hint = [n for n in numbers if n[1] >= last_hint_end]
        if after_hint:
            auth_value, auth_start, auth_end = after_hint[0]
        else:
            auth_value, auth_start, auth_end = numbers[-1]
    else:
        auth_value, auth_start, auth_end = numbers[-1]
    # Drop a number if it is preceded by a negation phrase within ~20 chars.
    pre = text[max(0, auth_start - 20):auth_start]
    if _NEG_BEFORE.search(pre):
        # Find the prior number that is not in a negation context.
        prior = [n for n in numbers if n[1] < auth_start]
        prior_clean = []
        for v, s, e in prior:
            pre2 = text[max(0, s - 20):s]
            if not _NEG_BEFORE.search(pre2):
                prior_clean.append((v, s, e))
        if prior_clean:
            auth_value, auth_start, auth_end = prior_clean[-1]
        else:
            # No clean prior; fall back to zero-word semantics if present.
            zw = _zero_word_value(text)
            auth_value = 0 if zw is not None else numbers[-1][0]

    if abs(auth_value - t) < 0.005:
        return 1.0
    # "None qualify" / "zero" patterns map to 0 — if the truth is 0 and the
    # user wrote that, treat as correct.
    if abs(t - 0) < 0.005 and _zero_word_value(text) is not None:
        if abs(auth_value - 0) < 0.005 or _zero_word_value(text) is not None:
            return 1.0
    # Correct figure present somewhere in the answer?
    if any(abs(v - t) < 0.005 for v, _, _ in numbers):
        return 0.3
    return 0.0

if __name__ == "__main__":
    for size in ("small", "medium", "large"):
        d = generate(0, size)
        print(f"[{size}] chars={d['meta']['n_chars']:,} "
              f"emails={d['meta']['n_emails']} reports={d['meta']['n_reports']}")
        print("  question:", d["question"][:90], "...")
        print("  truth:", d["answer"])
    t0 = generate(0, "small")["answer"]
    assert score(str(t0), t0) == 1.0
    t = generate(1, "small")["answer"]
    assert score(f"The total is {_usd(t)}, per my audit.", t) == 1.0
    assert score(f"answer: {t:,}", t) == 1.0
    # show_work: a correct final total must score 1.0 even when working
    # numbers appear earlier ("a + b = T" and "Total: T" both at the end).
    assert score(f"Qualifying: a + b = {t}. Total: {t}", t) == 1.0
    # A hedged answer naming a wrong total as THE answer must score < 1.
    assert score(f"The total is {t + 250} (not {t}).", t) < 1.0
    # wrong answers guaranteed distinct from the truth (t+0 trap removed)
    for wrong in (t + 1, t * 2 + 7, t + 913, t + 123_456):
        assert score(str(wrong), t) < 1.0
    assert score("I could not find any qualifying reports.", t) == 0.0
    print("self-checks passed")
