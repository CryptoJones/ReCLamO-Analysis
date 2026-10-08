import random
import re
import string
from typing import Dict, Any, Tuple, List

# Predefined pools for natural-sounding generation
FIRST_NAMES = ["Alex", "Blake", "Casey", "Dana", "Elliot", "Finley", "Gray", "Harper", "Avery", "Quinn", "Riley", "Taylor", "Jordan", "Morgan", "Jamie", "Peyton", "Reese", "Skyler", "Frankie", "Leslie"]
LAST_NAMES = ["Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis", "Rodriguez", "Martinez", "Hernandez", "Lopez", "Gonzalez", "Wilson", "Anderson", "Thomas", "Taylor", "Moore", "Jackson", "Martin"]
DEPTS = ["Product", "Engineering", "Design", "Data Science", "Marketing", "Legal", "AI Ethics", "Compliance", "Research", "Operations"]
PROJECTS = ["Nexus", "Atlas", "Orion", "Phoenix", "Helios", "Apex", "Vertex", "Summit", "Catalyst", "Pinnacle"]
MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
YEARS = [2023, 2024, 2025, 2026]

def _rand_name(rng: random.Random) -> str:
    return f"{rng.choice(FIRST_NAMES)} {rng.choice(LAST_NAMES)}"

def _rand_dept(rng: random.Random) -> str:
    return rng.choice(DEPTS)

def _rand_project(rng: random.Random) -> str:
    return rng.choice(PROJECTS)

def _rand_date(rng: random.Random, start_year: int = 2023, end_year: int = 2026) -> str:
    year = rng.randint(start_year, end_year)
    month = rng.choice(MONTHS)
    day = rng.randint(1, 28)
    return f"{month} {day}, {year}"

def _format_para(text: str) -> str:
    return text.strip() + "\n\n"

def generate(seed: int, size: str) -> Dict[str, Any]:
    rng = random.Random(seed)
    
    # Size to approximate character targets
    size_map = {"small": 60000, "medium": 300000, "large": 1200000}
    target_chars = size_map[size]
    
    # We'll build a sequence of email entries
    emails = []
    # State tracking for ground truth
    ai_ethics_owner = None
    ai_ethics_dept = None
    
    # Initial setup email
    initiator = _rand_name(rng)
    dept = _rand_dept(rng)
    project = _rand_project(rng)
    date = _rand_date(rng, 2023, 2023)
    emails.append(
        f"From: {initiator} <{initiator.lower().replace(' ', '.')}@company.com>\n"
        f"To: {dept} Team <{dept.lower()}@company.com>\n"
        f"Date: {date}\n"
        f"Subject: Kickoff: {project} AI Ethics Review\n\n"
        f"Hi team,\n\n"
        f"We're starting the AI Ethics Review for Project {project}. "
        f"This will be led by the {dept} department initially. "
        f"Please hold for further assignments.\n"
    )
    # Initially unassigned; will be assigned in follow-ups
    
    # Generate a series of updates, corrections, and redistributions
    # We'll aim for about 20-30 core events that affect ownership, plus filler
    num_core_events = 25 if size == "small" else 40 if size == "medium" else 60
    # We'll generate core events that may change AI Ethics ownership
    # and many filler emails about other topics
    
    # Track all people mentioned to avoid too many unique names
    people = {initiator}
    # Add some more people for variety
    for _ in range(15):
        people.add(_rand_name(rng))
    people = sorted(list(people))  # Sort for deterministic iteration
    
    # We'll generate a timeline of events from 2023 to 2026
    timeline = []
    # Start with the kickoff
    timeline.append(("2023-01-15", "kickoff", None, dept))  # (date, type, owner, dept)
    
    # Generate events that change ownership
    current_owner = None
    current_dept = dept
    
    for i in range(num_core_events):
        # Progressively advance date
        year = 2023 + (i * 2 // 12)  # Slowly advance years
        month = (i * 2) % 12 + 1
        if month > 12:
            month -= 12
            year += 1
        day = rng.randint(1, 28)
        date_str = f"{year}-{month:02d}-{day:02d}"
        readable_date = _rand_date(rng, year, year)  # Approximate
        
        event_type = rng.choice([
            "assign", "reassign", "decline", "cancel", "correct", "delegate", "note"
        ])
        
        if event_type == "assign" and current_owner is None:
            # Choose new owner from people, excluding initiator
            available_people = [p for p in people if p != initiator]
            new_owner = rng.choice(available_people)
            new_dept = _rand_dept(rng)
            timeline.append((date_str, "assign", new_owner, new_dept))
            current_owner = new_owner
            current_dept = new_dept
        elif event_type == "reassign" and current_owner is not None:
            # Sometimes reassign to same person but different dept (role change)
            if rng.random() < 0.3:
                new_dept = _rand_dept(rng)
                # Ensure dept change
                attempts = 0
                while new_dept == current_dept and attempts < 10:
                    new_dept = _rand_dept(rng)
                    attempts += 1
                if new_dept != current_dept:  # Only add if changed
                    timeline.append((date_str, "reassign", current_owner, new_dept))
                    current_dept = new_dept
            else:
                new_owner = rng.choice([p for p in people if p != current_owner])
                new_dept = _rand_dept(rng)
                timeline.append((date_str, "reassign", new_owner, new_dept))
                current_owner = new_owner
                current_dept = new_dept
        elif event_type == "decline" and current_owner is not None:
            timeline.append((date_str, "decline", current_owner, current_dept))
            current_owner = None  # Now unassigned
        elif event_type == "cancel" and current_owner is not None:
            timeline.append((date_str, "cancel", None, None))
            current_owner = None
            current_dept = None
        elif event_type == "correct" and current_owner is not None:
            # A correction that reaffirms or slightly adjusts
            if rng.random() < 0.5:
                # Just a reaffirmation
                timeline.append((date_str, "correct", current_owner, current_dept))
            else:
                # Correction to dept only
                new_dept = _rand_dept(rng)
                attempts = 0
                while new_dept == current_dept and attempts < 10:
                    new_dept = _rand_dept(rng)
                    attempts += 1
                if new_dept != current_dept:  # Only add if changed
                    timeline.append((date_str, "correct", current_owner, new_dept))
                    current_dept = new_dept
        # For delegate and note, we might not change ownership but generate email
        # We'll handle email generation separately
        
        # Also occasionally reset to unassigned then reassign later
        if rng.random() < 0.1 and current_owner is not None:
            timeline.append((date_str, "unassign", None, None))
            current_owner = None
            current_dept = None
    
    # Now generate actual email text for each timeline event, plus filler
    all_entries = []
    
    # Add the kickoff email as first entry (date fixed to match timeline[0][0]).
    _ky, _km, _kd = timeline[0][0].split("-")
    all_entries.append((
        timeline[0][0],  # date
        f"From: {initiator} <{initiator.lower().replace(' ', '.')}@company.com>\n"
        f"To: {dept} Team <{dept.lower()}@company.com>\n"
        f"Date: {MONTHS[int(_km) - 1]} {int(_kd)}, {_ky}\n"
        f"Subject: Kickoff: {project} AI Ethics Review\n\n"
        f"Hi team,\n\n"
        f"We're starting the AI Ethics Review for Project {project}. "
        f"This will be led by the {dept} department initially. "
        f"Please hold for further assignments.\n"
    ))
    
    # Process timeline events from index 1 onward
    for i in range(1, len(timeline)):
        date_str, etype, owner, dept_val = timeline[i]
        # Pick random people for from/to
        sender = rng.choice(people)
        # Recipient: sometimes team, sometimes individual
        if rng.random() < 0.4:
            recipient = f"{rng.choice(DEPTS).lower()} Team"
        else:
            recipient = rng.choice([p for p in people if p != sender])
        
        # Generate subject and body based on event type
        if etype == "assign":
            subject = f"Update: AI Ethics Review Lead Assigned"
            proj = rng.choice(PROJECTS)  # For variety in email
            body = (
                f"Hi {recipient.split()[0] if 'Team' not in recipient else 'team'},\n\n"
                f"I'm assigning the AI Ethics Review for Project {proj} to "
                f"{owner} from {dept_val}. Please direct all related questions to them.\n"
                f"Thanks,\n{sender}"
            )
        elif etype == "reassign":
            if owner is not None and (i == 1 or timeline[i-1][2] is not None):  # Person change
                subject = f"Update: AI Ethics Review Lead Changed"
                body = (
                    f"Hi team,\n\n"
                    f"Effective immediately, {owner} from {dept_val} will be taking over "
                    f"the AI Ethics Review responsibilities. "
                    f"The previous lead will no longer be involved.\n"
                    f"Please update your records.\n"
                    f"Thanks,\n{sender}"
                )
            else:  # Dept change only
                subject = f"Update: AI Ethics Review Department Change"
                body = (
                    f"Hi team,\n\n"
                    f"The AI Ethics Review will now be handled by the {dept_val} department, "
                    f"while {owner} remains the lead. "
                    f"This is to better align with compliance requirements.\n"
                    f"Thanks,\n{sender}"
                )
        elif etype == "decline":
            # Note: the truth computation treats "decline" as an ownership-changing event
            # that sets current_owner = None. The original email text said "Please reassign
            # this responsibility" but never said the position is now empty, so a careful
            # reader tracking only the most recent email could easily leave the original
            # lead in mind. Make it explicit so the email matches the truth.
            subject = f"Update: AI Ethics Review Lead Declined -- Position Now Unassigned"
            body = (
                f"Hi team,\n\n"
                f"I'm writing to confirm that the previous AI Ethics Review lead has declined "
                f"the role due to bandwidth constraints. Effective immediately, the AI Ethics "
                f"Review lead position is unassigned until a new lead can be confirmed. "
                f"Please update your records and re-circulate the call for a new lead.\n"
                f"Thanks,\n{sender}"
            )
        elif etype == "cancel":
            # Same as above: the truth is "Unassigned" after a cancel event; spell it out
            # in the body so the thread stays consistent with the key.
            subject = f"Update: AI Ethics Review Cancelled -- Position Now Unassigned"
            body = (
                f"Hi team,\n\n"
                f"The AI Ethics Review for Project {rng.choice(PROJECTS)} has been cancelled "
                f"and the lead position is now unassigned. No further action is required from "
                f"the previous reviewers and ownership will need to be re-confirmed before any "
                f"follow-up work begins.\n"
                f"Thanks,\n{sender}"
            )
        elif etype == "unassign":
            # Originally this fell through to the generic "Note: AI Ethics Review Update"
            # else-branch and produced an email that said "the AI Ethics Review is
            # progressing well. The team is handling the current deliverables.", which was
            # inconsistent with the truth key (which was now "Unassigned" because the
            # timeline's unassign event clears current_owner = None). The latest AI Ethics
            # email in the thread would then quietly disagree with the key. Add an explicit
            # branch that surfaces the unassigned state in the same prose style.
            subject = f"Update: AI Ethics Review Lead Unassigned"
            body = (
                f"Hi team,\n\n"
                f"This is to confirm that the AI Ethics Review lead position is now "
                f"unassigned. We are looking for a new lead to take over ownership and "
                f"will circulate a call for nominations shortly.\n"
                f"Thanks,\n{sender}"
            )
        elif etype == "correct":
            if rng.random() < 0.5:  # Reaffirmation
                subject = f"Confirmation: AI Ethics Review Ownership"
                body = (
                    f"Hi team,\n\n"
                    f"This is to confirm that {owner} from {dept_val} remains the lead "
                    f"for the AI Ethics Review. No changes have been made.\n"
                    f"Thanks,\n{sender}"
                )
            else:  # Dept correction
                subject = f"Correction: AI Ethics Review Department"
                body = (
                    f"Hi team,\n\n"
                    f"Please note a correction: the AI Ethics Review is under the {dept_val} department, "
                    f"not the previously mentioned one. {owner} remains the lead.\n"
                    f"Thanks,\n{sender}"
                )
        elif etype == "delegate":
            subject = f"Delegation: AI Ethics Review Task"
            body = (
                f"Hi {recipient.split()[0] if 'Team' not in recipient else 'team'},\n\n"
                f"I'm delegating the AI Ethics Review coordination to {owner} from {dept_val} "
                f"for the next phase. They will handle scheduling and documentation.\n"
                f"Thanks,\n{sender}"
            )
        else:  # note
            subject = f"Note: AI Ethics Review Update"
            body = (
                f"Hi team,\n\n"
                f"Quick note: the AI Ethics Review is progressing well. "
                f"{owner if owner else 'The team'} is handling the current deliverables.\n"
                f"Thanks,\n{sender}"
            )
        
        # FIX: the visible email date must match the timeline's date_str, otherwise
        # the rendered timeline order contradicts the date headers in the email
        # (the audit found 34% of seeds disagreed). Build a "Month DD, YYYY" form
        # directly from date_str so the two stay in lockstep.
        _year, _month, _day = date_str.split("-")
        date_readable = f"{MONTHS[int(_month) - 1]} {int(_day)}, {_year}"
        email_text = (
            f"From: {sender} <{sender.lower().replace(' ', '.')}@company.com>\n"
            f"To: {recipient}@company.com\n"
            f"Date: {date_readable}\n"
            f"Subject: {subject}\n\n"
            f"{body}\n"
        )
        all_entries.append((date_str, email_text))
    
    # Now add many filler emails about other topics to dilute the signal
    # We'll generate enough to reach target size
    filler_topics = [
        "Weekly sync", "Budget approval", "Hiring update", "Conference planning",
        "Vendor contract", "Office relocation", "IT outage", "Training session",
        "Performance review", "Holiday schedule", "Health benefits", "Travel policy"
    ]
    
    # We'll keep generating entries until we reach target size
    # First, sort all_entries by date (string comparison works for YYYY-MM-DD)
    all_entries.sort(key=lambda x: x[0])
    
    # Build the context by concatenating emails
    context_parts = []
    current_size = 0
    
    # Add all timeline-based emails
    for _, email_text in all_entries:
        context_parts.append(email_text)
        current_size += len(email_text)
    
    # Add filler emails until we reach target
    while current_size < target_chars:
        sender = rng.choice(people)
        # Recipient: team or individual
        if rng.random() < 0.5:
            recipient = f"{rng.choice(DEPTS).lower()} Team"
        else:
            recipient = rng.choice([p for p in people if p != sender])
        topic = rng.choice(filler_topics)
        project_filler = rng.choice(PROJECTS)
        date_filler = _rand_date(rng, 2023, 2026)
        
        filler_email = (
            f"From: {sender} <{sender.lower().replace(' ', '.')}@company.com>\n"
            f"To: {recipient.lower().replace(' ', '')}@company.com\n"
            f"Date: {date_filler}\n"
            f"Subject: {topic}: {project_filler}\n\n"
            f"Hi {recipient.split()[0] if 'Team' not in recipient else 'team'},\n\n"
            f"Just a quick update on the {topic.lower()} for {project_filler}. "
            f"We're on track for the milestones discussed last week. "
            f"Please let me know if you have any questions.\n"
            f"Thanks,\n{sender}\n"
        )
        context_parts.append(filler_email)
        current_size += len(filler_email)
    
    # Join all parts
    context = "".join(context_parts)
    
    # Trim to approximately target size (we might be slightly over)
    if len(context) > target_chars * 1.1:  # If way over, trim
        context = context[:target_chars]
        # Try to end at a clean boundary
        last_double_newline = context.rfind("\n\n")
        if last_double_newline > target_chars * 0.8:
            context = context[:last_double_newline]
    
    # Now, determine the ground truth: final owner of AI Ethics Review
    # We need to simulate the state changes from our timeline
    # Re-simulate to be sure
    final_owner = None
    final_dept = None
    
    # Process timeline in order
    for date_str, etype, owner, dept_val in timeline:
        if etype == "assign":
            final_owner = owner
            final_dept = dept_val
        elif etype == "reassign":
            final_owner = owner
            final_dept = dept_val
        elif etype == "decline":
            final_owner = None
            final_dept = None
        elif etype == "cancel":
            final_owner = None
            final_dept = None
        elif etype == "correct":
            # Corrections don't change owner unless specified (we handled dept changes in reassign/correct logic)
            # In our generation, correct only changes dept if we set it that way
            if owner is not None:  # If owner specified in correction
                final_owner = owner
                if dept_val is not None:
                    final_dept = dept_val
            # If dept_val changed but owner same, we need to update dept
            # Actually, in our timeline generation, for correct we sometimes changed dept
            # So we should update dept if dept_val is not None
            if dept_val is not None:
                final_dept = dept_val
        elif etype == "unassign":
            final_owner = None
            final_dept = None
    
    # The answer should be the final owner's name, or "Unassigned" if none
    if final_owner is None:
        ground_truth = "Unassigned"
    else:
        ground_truth = final_owner
    
    # Question
    question = (
        "After reviewing the entire email thread, who is the final owner responsible for the AI Ethics Review? "
        "If no one is currently assigned, respond with 'Unassigned'."
    )
    
    # Meta
    meta = {
        "seed": seed,
        "size": size,
        "context_chars": len(context),
        "num_emails": len(context_parts),
        "final_owner": final_owner,
        "final_dept": final_dept,
        "task_type": "temporal_coreference_resolution",
        "reasoning_required": [
            "track ownership changes over time",
            "handle corrections that override prior statements",
            "resolve coreferences like 'they', 'the lead', 'previous owner'",
            "ignore filler content about other topics",
            "integrate information distributed across the entire context"
        ]
    }
    
    return {
        "context": context,
        "question": question,
        "answer": ground_truth,
        "meta": meta
    }

def score(answer_text: str, truth) -> float:
    """Robust scorer for SELMA.

    - 1.0 when the answer unambiguously states the truth (case-insensitive,
      tolerant of formatting, prose wrapping, quotes, JSON, markdown, etc.)
      AND does NOT name a competing candidate as the "final" or "current" owner.
    - 0.3 when the answer hedges (names a competing candidate alongside the
      truth, or denies the truth while naming another).
    - 0.0 when nothing of substance is found, or a different name is named
      as the answer.
    """
    if not isinstance(answer_text, str) or not answer_text.strip():
        return 0.0

    text = answer_text.strip()
    text_low = text.lower()

    if truth == "Unassigned":
        truth_low = "unassigned"
        truth_words = ["unassigned"]
    else:
        truth_low = truth.lower().strip()
        truth_words = truth_low.split()

    # Detect hedge words that would suggest competing candidates.
    hedge_patterns = [
        r"\b(either|or|alternatively|possibly|probably|maybe|perhaps|"
        r"could be|might be|not\s+\w+)\b",
    ]
    has_hedge_word = any(
        re.search(pat, text_low) for pat in hedge_patterns
    )

    # For "Unassigned" truth, look for the word "unassigned" in the answer.
    if truth == "Unassigned":
        if re.search(r"\bunassigned\b", text_low):
            # If the answer also says "not unassigned" or hedges, drop below 1.0.
            if re.search(r"\bnot\s+unassigned\b|\bnever\s+unassigned\b", text_low):
                return 0.0
            if has_hedge_word:
                return 0.3
            return 1.0
        # "None" / "No one" / "nobody" / "empty" are also valid unassigned phrasings.
        if re.search(r"\b(none|no\s+one|nobody|nothing|empty|vacant|unfilled)\b", text_low):
            if has_hedge_word:
                return 0.3
            return 1.0
        # Otherwise wrong (the answer names someone or is gibberish).
        return 0.0

    # Truth is a name. Check for the full name (case-insensitive) anywhere in
    # the answer, OR last-first form.
    name_variants = [truth_low]
    if len(truth_words) == 2:
        last_first = truth_words[1] + ", " + truth_words[0]
        name_variants.append(last_first)
    truth_present = any(v in text_low for v in name_variants)

    if not truth_present:
        return 0.0

    # Find other candidate names mentioned in the answer (Title-Case multi-word).
    other_name_pattern = re.compile(
        r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+(?:-[A-Z][a-z]+)?)\b"
    )
    other_candidates = [
        n.lower() for n in other_name_pattern.findall(text)
    ]
    other_candidates = [
        n for n in other_candidates
        if n != truth_low and n not in name_variants
    ]

    # Check whether the answer says "the final owner is X" or "current owner is X"
    # pointing at a competing candidate.
    is_phrases = re.findall(
        r"\b(?:final|current|new|latest|now|true|actual|real|new)\s+"
        r"(?:final\s+)?owner\s+is\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+(?:-[A-Z][a-z]+)?)",
        text,
    )
    is_phrases_low = [s.lower() for s in is_phrases]
    # If the answer names a competing candidate as "the final/current owner",
    # the answer is wrong about the truth.
    if is_phrases_low and any(p != truth_low for p in is_phrases_low):
        return 0.0

    # If the answer says "moved from <truth> to <other>; the final owner is <other>"
    # this is a hedge that explicitly picks the wrong person.
    if other_candidates and has_hedge_word:
        # Look for a hedge that resolves to a competing candidate.
        denial = re.search(
            r"\bnot\s+" + re.escape(truth_low), text_low
        )
        if denial:
            return 0.0
        return 0.3

    return 1.0

if __name__ == "__main__":
    # Print stats for seed 0 at each size
    for size in ["small", "medium", "large"]:
        data = generate(0, size)
        print(f"=== {size.upper()} ===")
        print(f"Context length: {len(data['context']):,} characters")
        print(f"Question: {data['question']}")
        print(f"Ground truth answer: {data['answer']}")
        print(f"Meta: final owner={data['meta']['final_owner']}, dept={data['meta']['final_dept']}")
        print()
        
        # Verify scoring
        assert score(str(data["answer"]), data["answer"]) == 1.0, f"Self-score failed for {size}"
        
        # Test a few wrong answers
        wrong_answers = [
            "Unassigned" if data["answer"] != "Unassigned" else "Assigned",
            "Someone Else",
            "",
            "The team",
            "Alex Smith" if data["answer"] != "Alex Smith" else "Blake Taylor"
        ]
        for wrong in wrong_answers:
            s = score(wrong, data["answer"])
            assert s < 1.0, f"Wrong answer scored too high: '{wrong}' -> {s}"
            print(f"Wrong answer '{wrong}' scored: {s:.2f}")
        print("-" * 50)

    # ----- Defect-specific regression assertions -----
    # Original bug: a random `unassign` side-effect could fire after the last email about
    # AI Ethics, setting the truth key to "Unassigned" without producing any clear email
    # that said so; the same problem existed for the decline/cancel paths. The solver
    # looking at the latest AI Ethics email would still see a named lead in the
    # signature/declination text. The fix (1) makes unassign a dedicated email branch
    # that says "the lead position is now unassigned", and (2) makes decline/cancel
    # emails explicitly state that the position is now unassigned. This assertion proves
    # the TRUTH matches the LAST ownership-changing email for every (seed, size) cell.
    # Concretely: if the truth is "Unassigned", the most recent AI Ethics email must
    # mention "unassigned". If the truth is a name, that name must appear in the most
    # recent AI Ethics email (as the person who is now in the role).
    import re as _re_selma
    _SUBJ_PAT = _re_selma.compile(r"Subject:\s*([^\n]+)")
    _SUBJ_BACKSLASH = chr(92)
    for seed in range(10):
        for size in ["small", "medium", "large"]:
            d = generate(seed, size)
            ctx = d["context"]
            truth = d["answer"]
            # Find the most recent AI Ethics email by scanning from the end.
            last_email = None
            ix = len(ctx)
            while True:
                prev = ctx.rfind("From: ", 0, ix)
                if prev == -1:
                    break
                chunk = ctx[prev:ix]
                if "AI Ethics" in chunk:
                    last_email = chunk
                    break
                ix = prev
            assert last_email is not None, (
                "No AI Ethics email found at all (seed=" + str(seed) + ", size=" + size + ")"
            )
            _m = _SUBJ_PAT.search(last_email)
            subj_str = _m.group(1) if _m else "?"
            if truth == "Unassigned":
                assert "unassigned" in last_email.lower(), (
                    "SELMA defect: truth is 'Unassigned' but latest AI Ethics email "
                    "does not say 'unassigned' (seed=" + str(seed) + ", size=" + size + "). "
                    "Email subject: " + repr(subj_str)
                )
            else:
                assert truth in last_email, (
                    "SELMA defect: truth is " + repr(truth) + " but latest AI Ethics email "
                    "does not name them (seed=" + str(seed) + ", size=" + size + "). "
                    "Email subject: " + repr(subj_str)
                )
    print("SELMA: defect-specific assertions PASS (truth consistent with last ownership email).")

    # ----- Scorer regression: every format the audit probes must score 1.0 -----
    import json as _json_selma
    for seed in range(5):
        d = generate(seed, "small")
        truth = d["answer"]
        if truth == "Unassigned":
            positives = {
                "as_is": "Unassigned",
                "sentence": "The answer is Unassigned.",
                "bold": "**Unassigned**",
                "bold_in_sentence": "The owner is **Unassigned**.",
                "json": _json_selma.dumps({"owner": "Unassigned"}),
                "lead_in_explanation": "After applying every amendment and reversal, the owner is Unassigned.",
                "trailing_explanation": "Unassigned (after applying all corrections in the archive)",
                "final_answer_tag": "Final answer: Unassigned",
                "code_span": "`Unassigned`",
                "uppercase": "UNASSIGNED",
                "lowercase": "unassigned",
                "newline_after": "Unassigned\n",
                "quoted": '"Unassigned"',
                "sentence_currently": "The AI Ethics Review is currently unassigned.",
                "no_one": "No one is currently assigned (Unassigned).",
                "dash_explain": "Unassigned - the last owner was removed and nobody replaced them.",
                "final_owner_unassigned": "Final owner: Unassigned",
                "none": "None",
            }
        else:
            positives = {
                "as_is": truth,
                "sentence": f"The answer is {truth}.",
                "bold": f"**{truth}**",
                "bold_in_sentence": f"The owner is **{truth}**.",
                "json": _json_selma.dumps({"owner": truth}),
                "lead_in_explanation": f"After applying every amendment and reversal, the owner is {truth}.",
                "trailing_explanation": f"{truth} (after applying all corrections in the archive)",
                "final_answer_tag": f"Final answer: {truth}",
                "code_span": f"`{truth}`",
                "uppercase": truth.upper(),
                "lowercase": truth.lower(),
                "newline_after": f"{truth}\n",
                "quoted": f'"{truth}"',
                "final_owner_label": f"Final owner: {truth}",
                "with_dept": f"{truth} (Engineering)",
                "possessive": f"It is {truth}'s responsibility.",
                "md_bold_in_sentence": f"The final owner is **{truth}**.",
                "last_first": f"{truth.split()[-1]}, {truth.split()[0]}",
                "quoted_in_sentence": f"The final owner is \"{truth}\".",
                "name_then_comma": f"{truth}, who took over after the last reassignment.",
            }
        for k, s in positives.items():
            sc = score(s, d["answer"])
            assert sc == 1.0, (
                f"SELMA positive {k!r} should score 1.0 (seed={seed}); got {sc}"
            )
    print("SELMA: scorer regression battery PASS (formats in).")
