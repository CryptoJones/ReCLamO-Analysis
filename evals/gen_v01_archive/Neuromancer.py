import random
import re
from typing import Dict, Any

def generate(seed: int, size: str) -> Dict[str, Any]:
    rng = random.Random(seed)
    
    # Size parameters
    size_params = {
        "small":  (20, 60000),
        "medium": (100, 300000),
        "large":  (400, 1200000)
    }
    num_entries, target_chars = size_params[size]
    
    # ---------- Data generation ----------
    employees = ["Alice", "Bob", "Carol", "Dave", "Eve", "Frank", "Grace", "Hank", "Ivy", "Jack"]
    categories = ["Travel", "Office Supplies", "Meals", "Entertainment"]
    months = ["January", "February", "March", "April", "May", "June"]
    
    entries = []
    for _ in range(num_entries):
        emp = rng.choice(employees)
        month = rng.choice(months)
        cat = rng.choice(categories)
        orig_amount = round(rng.uniform(50, 5000), 2)
        # Generate adjustments (0 to 2)
        num_adjustments = rng.randint(0, 2)
        adjustments = []
        for _ in range(num_adjustments):
            adj_amount = round(rng.uniform(-orig_amount*0.5, orig_amount*0.5), 2)
            adjustments.append(adj_amount)
        # Final amount after adjustments
        final_amount = max(0, orig_amount + sum(adjustments))
        # Status: denied if final amount becomes 0 or randomly
        if final_amount == 0 or rng.random() < 0.15:
            status = "Denied"
            final_amount = 0.0
        else:
            status = "Approved"
        entries.append({
            "employee": emp,
            "month": month,
            "category": cat,
            "orig_amount": orig_amount,
            "adjustments": adjustments,
            "final_amount": final_amount,
            "status": status
        })
    
    # ---------- Text generation ----------
    # Templates with synonyms and coreference
    submission_templates = [
        "From: {emp}\nTo: Finance\nSubject: {month} {cat}\nI have submitted my {cat} expenses for {month}. The total was ${amt:.2f}. Please process.\n",
        "To: Finance\nFrom: {emp}\nRe: {month} {cat} Claim\nAttached is my {cat} request for {month}. Amount: ${amt:.2f}. Kindly approve.\n",
        "Subject: {month} {cat} Reimbursement\nFrom: {emp}\nFinance team,\nPlease find my {cat} expenses for {month} totaling ${amt:.2f}. Thanks.\n"
    ]
    adjustment_templates = [
        "From: Finance\nTo: {emp}\nRe: {month} {cat}\nWe reviewed your claim. Due to policy change, the amount is reduced by ${adj:.2f}. New total: ${new_amt:.2f}.\n",
        "From: {emp}\nTo: Finance\nSubject: Correction to {month} {cat}\nI made an error. The correct amount should be ${new_amt:.2f}, not ${orig_amt:.2f}.\n",
        "From: Manager\nTo: Finance\nRe: {emp}'s {month} {cat}\n{emp}'s trip was partly personal. Deduct ${adj:.2f}. Adjusted amount: ${new_amt:.2f}.\n"
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
    
    # Build text fragments
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
        
        # Adjustments (if any)
        running_amt = orig
        for adj in adjustments:
            running_amt += adj
            new_amt = max(0, running_amt)
            adj_text = rng.choice(adjustment_templates).format(
                emp=emp, month=month, cat=cat, adj=abs(adj), new_amt=new_amt, orig_amt=orig
            )
            fragments.append(adj_text)
        
        # Final status
        if status == "Denied":
            den_text = rng.choice(denial_templates).format(emp=emp, month=month, cat=cat)
            fragments.append(den_text)
        else:
            app_text = rng.choice(approval_templates).format(emp=emp, month=month, cat=cat, amt=final)
            fragments.append(app_text)
    
    # Add filler to reach target length (roughly)
    current_len = sum(len(f) for f in fragments)
    target_len = target_chars - 5000  # leave room for question and meta
    while current_len < target_len:
        filler = rng.choice(filler_templates).format(emp=rng.choice(employees), month=rng.choice(months))
        fragments.append(filler)
        current_len += len(filler)
    
    # Shuffle fragments to interleave and make it harder to track
    rng.shuffle(fragments)
    context = "".join(fragments)
    # Trim to target if slightly over (rough)
    if len(context) > target_chars:
        context = context[:target_chars]
    
    # ---------- Answer computation ----------
    # Total reimbursed for March travel entries that were not denied
    total = 0.0
    for entry in entries:
        if entry["month"] == "March" and entry["category"] == "Travel" and entry["status"] != "Denied":
            total += entry["final_amount"]
    answer = f"${total:.2f}"
    
    # Question (fixed)
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
    # truth is a string like "$123.45"
    # Extract numeric value from answer_text (robust to formatting)
    # Use regex to find a dollar amount (optional $, commas, decimal)
    pattern = r'\$?\s*([\d,]+(?:\.\d{1,2})?)'
    matches = re.findall(pattern, answer_text)
    if not matches:
        return 0.0
    # Take the last match? Or the first? We'll take the first numeric that matches the truth format.
    # Better: try to find the exact truth number in the answer text (ignoring commas and $)
    truth_num = float(truth.replace("$", "").replace(",", ""))
    for m in matches:
        num_str = m.replace(",", "")
        try:
            num = float(num_str)
            if abs(num - truth_num) < 0.01:
                return 1.0
        except:
            continue
    # If no exact match, check if answer_text contains the truth as a substring after normalization
    norm_answer = answer_text.replace("$", "").replace(",", "").lower().strip()
    norm_truth = truth.replace("$", "").replace(",", "").lower().strip()
    if norm_truth in norm_answer:
        return 1.0
    return 0.0


if __name__ == "__main__":
    for size in ["small", "medium", "large"]:
        result = generate(0, size)
        ctx = result["context"]
        q = result["question"]
        ans = result["answer"]
        meta = result["meta"]
        print(f"Size: {size}")
        print(f"Context length: {len(ctx)} chars")
        print(f"Question: {q}")
        print(f"Truth answer: {ans}")
        print(f"Meta: {meta}")
        # Test score
        assert score(ans, ans) == 1.0, f"Score of truth should be 1.0, got {score(ans, ans)}"
        # Test some wrong answers
        wrong1 = "$0.00"
        wrong2 = "I don't know"
        wrong3 = ans.replace("$", "")  # missing dollar sign but number same
        # wrong3 should still score 1.0 because we ignore $?
        # Actually our score function should handle that. Let's test.
        print(f"Score for truth: {score(ans, ans)}")
        print(f"Score for wrong1 ($0.00): {score(wrong1, ans)}")
        print(f"Score for wrong2: {score(wrong2, ans)}")
        print(f"Score for wrong3 (no $): {score(wrong3, ans)}")
        assert score(wrong1, ans) < 1.0
        assert score(wrong2, ans) < 1.0
        # wrong3 should be 1.0 because number is same
        assert score(wrong3, ans) == 1.0, f"Expected 1.0 for missing $, got {score(wrong3, ans)}"
        print("All assertions passed.\n")
