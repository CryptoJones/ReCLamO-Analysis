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

    # delegation: one Operations manager delegates approvals while away
    ops_mgrs = [m for m in mgrs if div_of[m] == "Operations"]
    deputy = None
    if ops_mgrs:
        delegator = ops_mgrs[0]
        deputy = rng.choice([e for e in employees
                             if div_of[e] != "Operations"])
        # reports under delegator switch approver to deputy (NOT Operations)
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
                   f"{r['cat']} purchases ({rng.choice(['the', 'this month\u2019s', 'routine'])} batch)")
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

def score(answer_text: str, truth) -> float:
    """Robust to prose around the number; not lenient about substance."""
    try:
        truth = float(truth)
    except (TypeError, ValueError):
        return 0.0
    for tok in _NUM.findall(str(answer_text or "")):
        try:
            v = float(tok.replace(",", ""))
        except ValueError:
            continue
        if abs(v - truth) < 0.005:
            return 1.0
    return 0.0

if __name__ == "__main__":
    for size in ("small", "medium", "large"):
        d = generate(0, size)
        print(f"[{size}] chars={d['meta']['n_chars']:,} "
              f"emails={d['meta']['n_emails']} reports={d['meta']['n_reports']}")
        print("  question:", d["question"][:90], "...")
        print("  truth:", d["answer"])
    assert score(str(generate(0, "small")["answer"]),
                 generate(0, "small")["answer"]) == 1.0
    t = generate(1, "small")["answer"]
    assert score(f"The total is {_usd(t)}, per my audit.", t) == 1.0
    for wrong in (t + 1, t * 2 + 7, 0, max(t - 500, 1)):
        assert score(str(wrong), t) < 1.0
    print("self-checks passed")
