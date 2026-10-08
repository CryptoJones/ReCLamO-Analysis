import random
import re
from typing import Dict, Any, List, Tuple

def generate(seed: int, size: str) -> Dict[str, Any]:
    rng = random.Random(seed)

    # Size parameters: (total entries, target chars)
    size_params = {
        "small":  (30, 60000),
        "medium": (120, 300000),
        "large":  (500, 1200000)
    }
    num_entries, target_chars = size_params[size]

    # ---------- Data generation ----------
    employees = ["Alice", "Bob", "Carol", "Dave", "Eve", "Frank", "Grace", "Hank", "Ivy", "Jack"]
    categories = ["Travel", "Office Supplies", "Meals", "Entertainment"]
    months = ["January", "February", "March", "April", "May", "June"]

    # Force at least one March travel entry with non-zero final amount (not denied)
    num_forced = {"small": 2, "medium": 5, "large": 10}[size]
    forced_entries = []
    for _ in range(num_forced):
        emp = rng.choice(employees)
        month = "March"
        cat = "Travel"
        orig_amount = round(rng.uniform(100, 5000), 2)
        adj = round(rng.uniform(-orig_amount*0.3, orig_amount*0.3), 2)
        final_amount = max(1.0, orig_amount + adj)
        forced_entries.append({
            "employee": emp,
            "month": month,
            "category": cat,
            "orig_amount": orig_amount,
            "adjustments": [adj],
            "final_amount": final_amount,
            "status": "Approved"
        })

    # Generate remaining entries randomly
    remaining_entries = []
    for _ in range(num_entries - num_forced):
        emp = rng.choice(employees)
        month = rng.choice(months)
        cat = rng.choice(categories)
        orig_amount = round(rng.uniform(50, 5000), 2)
        num_adjustments = rng.randint(0, 2)
        adjustments = []
        for _ in range(num_adjustments):
            adj_amount = round(rng.uniform(-orig_amount*0.5, orig_amount*0.5), 2)
            adjustments.append(adj_amount)
        final_amount = max(0, orig_amount + sum(adjustments))
        if final_amount == 0 or rng.random() < 0.15:
            status = "Denied"
            final_amount = 0.0
        else:
            status = "Approved"
        remaining_entries.append({
            "employee": emp,
            "month": month,
            "category": cat,
            "orig_amount": orig_amount,
            "adjustments": adjustments,
            "final_amount": final_amount,
            "status": status
        })

    entries = forced_entries + remaining_entries
    rng.shuffle(entries)

    # ---------- Text generation ----------
    submission_templates = [
        "From: {emp}\nTo: Finance\nSubject: {month} {cat}\nI have submitted my {cat} expenses for {month}. The total was ${amt:.2f}. Please process.\n",
        "To: Finance\nFrom: {emp}\nRe: {month} {cat} Claim\nAttached is my {cat} request for {month}. Amount: ${amt:.2f}. Kindly approve.\n",
        "Subject: {month} {cat} Reimbursement\nFrom: {emp}\nFinance team,\nPlease find my {cat} expenses for {month} totaling ${amt:.2f}. Thanks.\n"
    ]

    # Adjustment templates – now direction‑aware
    inc_templates = [
        "From: Finance\nTo: {emp}\nRe: {month} {cat}\nWe reviewed your claim. Due to a policy change, the amount is increased by ${abs_adj:.2f}. New total: ${new_amt:.2f}.\n",
        "From: {emp}\nTo: Finance\nSubject: Correction to {month} {cat}\nI made an error. The correct amount should be ${new_amt:.2f}, not ${orig_amt:.2f}. (This is an increase.)\n",
        "From: Manager\nTo: Finance\nRe: {emp}'s {month} {cat}\n{emp}'s trip was partly business. Add ${abs_adj:.2f}. Adjusted amount: ${new_amt:.2f}.\n"
    ]
    dec_templates = [
        "From: Finance\nTo: {emp}\nRe: {month} {cat}\nWe reviewed your claim. Due to a policy change, the amount is reduced by ${abs_adj:.2f}. New total: ${new_amt:.2f}.\n",
        "From: {emp}\nTo: Finance\nSubject: Correction to {month} {cat}\nI made an error. The correct amount should be ${new_amt:.2f}, not ${orig_amt:.2f}. (This is a reduction.)\n",
        "From: Manager\nTo: Finance\nRe: {emp}'s {month} {cat}\n{emp}'s trip was partly personal. Deduct ${abs_adj:.2f}. Adjusted amount: ${new_amt:.2f}.\n"
    ]

    denial_templates = [
        "From: Finance\nTo: {emp}\nRe: {month} {cat}\nYour claim is denied because it does not meet policy. No reimbursement.\n",
        "From: Manager\nTo: Finance\nSubject: Deny {emp}'s {month} {cat}\nPlease reject the latter claim. Not approved.\n",
        "From: {emp}\nTo: Finance\nSubject: Withdrawal of {month} {cat}\nI am withdrawing my request. It should be canceled.\n"
    ]
    approval_templates = [
        "From: Finance\nTo: {emp}\nRe: {month} {cat}\nYour claim is approved for ${amt:.2f}. Payment will be issued.\n",
        "From: Finance\nTo: {emp}\nSubject: Approved: {month} {cat}\nWe have approved ${amt:.2f} for your {cat} expenses.\n"
    ]
    filler_templates = [
        "From: IT\nTo: All\nSubject: Network maintenance\nPlease be aware that the network will be down this weekend.\n",
        "From: HR\nTo: All\nSubject: Office party\nJoin us for the annual office party on Friday.\n",
        "From: {emp}\nTo: Manager\nSubject: Vacation request\nI would like to request vacation days in {month}.\n",
        "From: Finance\nTo: {emp}\nSubject: Payroll update\nYour payroll for this month has been processed.\n",
        "To: All\nFrom: CEO\nSubject: Quarterly goals\nLet's achieve our Q2 targets.\n"
    ]

    fragments = []
    for entry in entries:
        emp = entry["employee"]
        month = entry["month"]
        cat = entry["category"]
        orig = entry["orig_amount"]
        adjustments = entry["adjustments"]
        final = entry["final_amount"]
        status = entry["status"]

        # Submission
        sub = rng.choice(submission_templates).format(emp=emp, month=month, cat=cat, amt=orig)
        fragments.append(sub)

        # Adjustments
        running_amt = orig
        for adj in adjustments:
            running_amt += adj
            new_amt = max(0, running_amt)
            abs_adj = abs(adj)
            if adj >= 0:
                adj_text = rng.choice(inc_templates).format(
                    emp=emp, month=month, cat=cat, abs_adj=abs_adj, new_amt=new_amt, orig_amt=orig
                )
            else:
                adj_text = rng.choice(dec_templates).format(
                    emp=emp, month=month, cat=cat, abs_adj=abs_adj, new_amt=new_amt, orig_amt=orig
                )
            fragments.append(adj_text)

        # Final status
        if status == "Denied":
            den_text = rng.choice(denial_templates).format(emp=emp, month=month, cat=cat)
            fragments.append(den_text)
        else:
            app_text = rng.choice(approval_templates).format(emp=emp, month=month, cat=cat, amt=final)
            fragments.append(app_text)

    # Add filler to reach target length
    current_len = sum(len(f) for f in fragments)
    target_len = target_chars - 5000
    while current_len < target_len:
        filler = rng.choice(filler_templates).format(emp=rng.choice(employees), month=rng.choice(months))
        fragments.append(filler)
        current_len += len(filler)

    rng.shuffle(fragments)
    context = "".join(fragments)
    if len(context) > target_chars:
        context = context[:target_chars]

    # ---------- Answer computation ----------
    total = 0.0
    for entry in entries:
        if entry["month"] == "March" and entry["category"] == "Travel" and entry["status"] != "Denied":
            total += entry["final_amount"]
    answer = f"${total:.2f}"

    question = "What is the total amount reimbursed for travel expenses that were originally submitted in March, after all adjustments, excluding any that were ultimately denied?"

    meta = {
        "num_entries": len(entries),
        "num_march_travel": sum(1 for e in entries if e["month"]=="March" and e["category"]=="Travel"),
        "num_denied_march_travel": sum(1 for e in entries if e["month"]=="March" and e["category"]=="Travel" and e["status"]=="Denied"),
        "total_approved_march_travel": total,
        "size": size,
        "context_length": len(context)
    }

    return {"context": context, "question": question, "answer": answer, "meta": meta}


def score(answer_text: str, truth: Any) -> float:
    # Extract all dollar amounts (with optional $, commas, decimal)
    pattern = r'\$?\s*([\d,]+(?:\.\d{1,2})?)'
    matches = re.findall(pattern, answer_text)
    if not matches:
        return 0.0

    truth_str = truth.replace("$", "").replace(",", "")
    try:
        truth_num = float(truth_str)
    except:
        return 0.0

    # Collect distinct numeric values (as floats)
    amounts = []
    for m in matches:
        num_str = m.replace(",", "")
        try:
            num = float(num_str)
            amounts.append(round(num, 2))
        except:
            continue

    if not amounts:
        return 0.0

    # Check for hedges: if the answer contains a negated amount or an "or" between amounts
    # We scan for negation patterns: "not $X", "no $X", "incorrect $X", "wrong $X", "instead of $X", "rather than $X"
    # Also amounts separated by "or" (e.g., "$X or $Y")
    hedge_patterns = [
        r'(?:not|no|never|wrong|incorrect|instead\s+of|rather\s+than)\s+\$?\s*[\d,]+(?:\.\d{1,2})?',
        r'\$?\s*[\d,]+(?:\.\d{1,2})?\s+or\s+\$?\s*[\d,]+(?:\.\d{1,2})?',
        r'\$?\s*[\d,]+(?:\.\d{1,2})?\s+and\s+\$?\s*[\d,]+(?:\.\d{1,2})?\s+are\s+both\s+wrong',
    ]
    for hp in hedge_patterns:
        if re.search(hp, answer_text, re.IGNORECASE):
            return 0.0

    # Also, if there are multiple distinct amounts and any of them is not equal to truth,
    # and the answer contains a word like "or", "not", "maybe", "possibly", "either", "neither",
    # we treat it as a hedge. This catches more subtle cases.
    distinct_amounts = sorted(set(amounts))
    if len(distinct_amounts) > 1:
        # Check for hedging keywords
        hedge_keywords = r'\b(?:or|not|maybe|possibly|either|neither|instead|rather|wrong|incorrect)\b'
        if re.search(hedge_keywords, answer_text, re.IGNORECASE):
            # If truth is among the amounts, but there is a wrong amount, it's a hedge
            if truth_num in distinct_amounts:
                if any(abs(a - truth_num) > 0.01 for a in distinct_amounts):
                    return 0.0

    # Now check if truth appears as a distinct amount
    if any(abs(a - truth_num) < 0.01 for a in distinct_amounts):
        # Ensure the answer contains the truth substring (normalized) as a safeguard
        norm_answer = answer_text.replace("$", "").replace(",", "").lower().strip()
        norm_truth = truth.replace("$", "").replace(",", "").lower().strip()
        if norm_truth in norm_answer:
            return 1.0

    return 0.0


if __name__ == "__main__":
    for seed in range(5):
        for size in ["small", "medium", "large"]:
            result = generate(seed, size)
            ctx = result["context"]
            q = result["question"]
            ans = result["answer"]
            meta = result["meta"]
            print(f"Seed {seed}, Size {size}: context length {len(ctx)} chars, truth {ans}")

            # Basic correctness
            assert score(ans, ans) == 1.0, f"Truth should score 1.0, got {score(ans, ans)}"

            # Test various correct formats (should all score 1.0)
            truth_num = float(ans.replace("$", "").replace(",", ""))
            correct_formats = [
                f"${truth_num:.2f}",                                          # exact
                f"${truth_num:.2f}.",                                        # trailing period
                f"The answer is ${truth_num:.2f}",                           # sentence
                f"**${truth_num:.2f}**",                                     # markdown bold
                f"*${truth_num:.2f}*",                                       # markdown italics
                f"`${truth_num:.2f}`",                                       # code
                f"\"${truth_num:.2f}\"",                                     # quotes
                f"${truth_num:,.2f}",                                        # with thousands comma
                f"{truth_num:.2f}",                                          # no dollar sign
                f"The total reimbursed amount is ${truth_num:.2f}.",         # extra prose before
                f"Answer: ${truth_num:.2f}.",                                # label
                f"{{ \"amount\": ${truth_num:.2f} }}",                      # JSON-like
                f"- ${truth_num:.2f}\n- Other stuff",                        # bullet list with extra
            ]
            for fmt in correct_formats:
                sc = score(fmt, ans)
                assert sc == 1.0, f"Format '{fmt}' should score 1.0, got {sc}"

            # Test hedges (should score 0.0)
            hedge_formats = [
                f"Not ${truth_num+1:.2f}; it is ${truth_num:.2f}",
                f"${truth_num+1:.2f} or ${truth_num:.2f}",
                f"Maybe ${truth_num:.2f} or ${truth_num+2:.2f}",
                f"The answer is not ${truth_num+5:.2f}, it is ${truth_num:.2f}",
                f"Either ${truth_num:.2f} or ${truth_num+1:.2f}",
                f"Incorrect: ${truth_num+10:.2f}; correct: ${truth_num:.2f}",
                f"${truth_num:.2f} and ${truth_num+3:.2f} are both wrong",
            ]
            for hedge in hedge_formats:
                sc = score(hedge, ans)
                assert sc == 0.0, f"Hedge '{hedge}' should score 0.0, got {sc}"

            # Test clearly wrong answers
            wrongs = [
                "$0.00",
                "I don't know",
                "No idea",
                f"${truth_num+100:.2f}",
                "None",
                "zero",
                "The amount is $999.99",
            ]
            for w in wrongs:
                sc = score(w, ans)
                assert sc < 1.0, f"Wrong answer '{w}' should score <1.0, got {sc}"

            # Test that answer with only truth and no extra amounts works
            extra_prose = f"After reviewing all records, the total reimbursement for March travel is ${truth_num:.2f}. This includes adjustments and excludes denials."
            assert score(extra_prose, ans) == 1.0, f"Extra prose should score 1.0, got {score(extra_prose, ans)}"

        print(f"Seed {seed}: all assertions passed.\n")
    print("All seeds and sizes passed.")
