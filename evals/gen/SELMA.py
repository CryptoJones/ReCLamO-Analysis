import random
import re
import string
from typing import Dict, Any, Tuple

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
    people = list(people)
    
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
            new_owner = rng.choice([p for p in people if p != initiator])
            new_dept = _rand_dept(rng)
            timeline.append((date_str, "assign", new_owner, new_dept))
            current_owner = new_owner
            current_dept = new_dept
        elif event_type == "reassign" and current_owner is not None:
            # Sometimes reassign to same person but different dept (role change)
            if rng.random() < 0.3:
                new_dept = _rand_dept(rng)
                while new_dept == current_dept:
                    new_dept = _rand_dept(rng)
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
                while new_dept == current_dept:
                    new_dept = _rand_dept(rng)
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
    
    # Add the kickoff email as first entry
    all_entries.append((
        timeline[0][0],  # date
        f"From: {initiator} <{initiator.lower().replace(' ', '.')}@company.com>\n"
        f"To: {dept} Team <{dept.lower()}@company.com>\n"
        f"Date: {_rand_date(rng, 2023, 2023)}\n"
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
            body = (
                f"Hi {recipient.split()[0] if 'Team' not in recipient else 'team'},\n\n"
                f"I'm assigning the AI Ethics Review for Project {rng.choice(PROJECTS)} to "
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
            subject = f"Update: Unable to Lead AI Ethics Review"
            body = (
                f"Hi {recipient.split()[0] if 'Team' not in recipient else 'team'},\n\n"
                f"Unfortunately, I need to decline leading the AI Ethics Review due to bandwidth constraints. "
                f"Please reassign this responsibility.\n"
                f"Thanks,\n{sender}"
            )
        elif etype == "cancel":
            subject = f"Update: AI Ethics Review Postponed"
            body = (
                f"Hi team,\n\n"
                f"The AI Ethics Review for Project {rng.choice(PROJECTS)} has been postponed indefinitely. "
                f"No further action is required at this time.\n"
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
        
        date_readable = _rand_date(rng, int(date_str[:4]), int(date_str[:4]))
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
        recipient = rng.choice([f"{dept} Team" for dept in DEPTS] + [p for p in people if p != sender])
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
    """
    Score the answer against the ground truth.
    - Case-insensitive
    - Ignores surrounding punctuation and whitespace
    - Ignores extra prose before/after but requires the core answer to be present
    - For names: must match the exact person (first + last) as assigned
    - "Unassigned" is a special case
    """
    if not isinstance(answer_text, str):
        return 0.0
    
    # Normalize: lower case, strip whitespace, remove punctuation at edges
    normalized = answer_text.strip().lower()
    # Remove common punctuation that might surround the answer
    normalized = normalized.strip(string.punctuation + string.whitespace)
    
    # Handle truth
    if truth == "Unassigned":
        expected = "unassigned"
    else:
        # Expected is the full name in lower case
        expected = truth.lower()
    
    # Check for exact match (after normalization)
    if normalized == expected:
        return 1.0
    
    # Allow for common variations: missing middle initial, etc. but we don't generate those
    # Also allow if the answer is embedded in a sentence but we require it to be the core answer
    # We'll check if the normalized string equals expected, or if it's a sentence that ends/starts with it
    # But to avoid being too lenient, we'll require that after stripping, it matches
    # However, we should allow for punctuation like periods at the end
    # We already stripped punctuation, so if they wrote "Alex Smith." it becomes "alex smith"
    
    # Additionally, check if the expected string is a substring and the answer is short
    # But to avoid false positives (e.g., "alex" matching "alex smith"), we require word boundaries
    # Since we normalized, we can split and check
    if expected == "unassigned":
        # Check if the answer is exactly unassigned, possibly with extra words
        # We'll be strict: must be exactly unassigned
        if normalized == "unassigned":
            return 1.0
        # But allow common phrases like "The owner is unassigned" -> we want to detect that
        # So we'll check if the normalized string contains "unassigned" and is not too long
        words = normalized.split()
        if len(words) <= 3 and "unassigned" in words:
            # Check that it's not like "assigned" or "unassignedly"
            for i, w in enumerate(words):
                if w == "unassigned":
                    # Check surroundings
                    if (i == 0 or words[i-1] in ["the", "is", "was", "current", "final"]) and \
                       (i == len(words)-1 or words[i+1] in [".", "!", "", "is", "was"]):
                        return 1.0
            # If we get here, it's ambiguous
            return 0.0
    else:
        # For names: check if the normalized answer matches the expected name
        # We'll split into words and check if all expected words appear in order
        expected_words = expected.split()
        answer_words = normalized.split()
        
        # Check for exact sequence
        if answer_words == expected_words:
            return 1.0
        
        # Check if answer_words contains expected_words as a subsequence (in order)
        # But we don't want to allow missing words (e.g., just "Alex" for "Alex Smith")
        # So we require all expected words to be present
        if all(word in answer_words for word in expected_words):
            # Now check if they appear in the right order (simple version: find first occurrence of each)
            # This is not perfect but good enough for our case
            try:
                positions = []
                for word in expected_words:
                    pos = answer_words.index(word)
                    positions.append(pos)
                # Check if positions are increasing
                if all(positions[i] <= positions[i+1] for i in range(len(positions)-1)):
                    # Additionally, check that there are no extra words that change meaning
                    # We'll be lenient: allow extra words at beginning or end
                    # But not in the middle that would break the name
                    # Actually, if they wrote "Alex Jay Smith" for "Alex Smith", that's wrong
                    # So we need to ensure no extra words between the expected words
                    # Re-check: the sequence must be contiguous
                    # Find where the subsequence starts
                    for start in range(len(answer_words) - len(expected_words) + 1):
                        if answer_words[start:start+len(expected_words)] == expected_words:
                            return 1.0
            except ValueError:
                pass
    
    # If we get here, no match
    return 0.0

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
