#!/usr/bin/env python3
"""
Validate a SKILL.md file against the Agent Skills specification.
Usage: python validate_skill.py path/to/skill-directory
"""

import sys
import os
import re

def validate_skill(skill_dir):
    """Validate a skill directory and its SKILL.md file."""
    errors = []
    warnings = []

    # Check directory exists
    if not os.path.isdir(skill_dir):
        errors.append(f"Directory not found: {skill_dir}")
        return errors, warnings

    skill_md = os.path.join(skill_dir, "SKILL.md")
    if not os.path.isfile(skill_md):
        errors.append("SKILL.md not found in skill directory")
        return errors, warnings

    with open(skill_md, "r", encoding="utf-8") as f:
        content = f.read()

    # Parse frontmatter
    fm_match = re.match(r'^---\s*\n(.*?)\n---\s*\n', content, re.DOTALL)
    if not fm_match:
        errors.append("Missing or malformed YAML frontmatter (must start with --- and end with ---)")
        return errors, warnings

    frontmatter = fm_match.group(1)
    body = content[fm_match.end():]

    # Check name field
    name_match = re.search(r'^name:\s*(.+)$', frontmatter, re.MULTILINE)
    if not name_match:
        errors.append("Missing required 'name' field in frontmatter")
    else:
        name = name_match.group(1).strip().strip('"').strip("'")
        if len(name) > 64:
            errors.append(f"Name exceeds 64 characters ({len(name)} chars): {name}")
        if not re.match(r'^[a-z0-9]([a-z0-9-]*[a-z0-9])?$', name):
            errors.append(f"Name must be lowercase alphanumeric with hyphens, no leading/trailing hyphens: {name}")
        if '--' in name:
            errors.append(f"Name must not contain consecutive hyphens: {name}")

        # Check directory name matches
        dir_name = os.path.basename(os.path.normpath(skill_dir))
        if dir_name != name:
            warnings.append(f"Directory name '{dir_name}' does not match skill name '{name}'")

    # Check description field
    desc_match = re.search(r'^description:\s*(.+)$', frontmatter, re.MULTILINE)
    if not desc_match:
        errors.append("Missing required 'description' field in frontmatter")
    else:
        desc = desc_match.group(1).strip().strip('"').strip("'")
        if len(desc) > 200:
            warnings.append(f"Description exceeds 200 characters ({len(desc)} chars) — may be truncated on some platforms")
        if len(desc) < 30:
            warnings.append(f"Description is very short ({len(desc)} chars) — may not trigger reliably")

    # Check body content
    lines = body.strip().split('\n')
    line_count = len(lines)

    if line_count > 500:
        warnings.append(f"SKILL.md body is {line_count} lines — recommended max is 500. Consider splitting into references/")

    if line_count < 5:
        warnings.append(f"SKILL.md body is only {line_count} lines — may lack sufficient instructions")

    # Check for common sections
    has_workflow = bool(re.search(r'^##\s*(Workflow|Steps|Process|How)', body, re.MULTILINE | re.IGNORECASE))
    has_examples = bool(re.search(r'^##\s*(Example|Usage)', body, re.MULTILINE | re.IGNORECASE))
    has_output = bool(re.search(r'^##\s*(Output|Format|Template)', body, re.MULTILINE | re.IGNORECASE))

    if not has_workflow:
        warnings.append("No 'Workflow' or 'Steps' section found — consider adding one")
    if not has_examples:
        warnings.append("No 'Examples' section found — examples improve agent output quality")
    if not has_output:
        warnings.append("No 'Output Format' section found — defining output format reduces ambiguity")

    # Check for autonomy anti-patterns
    ask_patterns = re.findall(r'ask\s+the\s+user|ask\s+for\s+clarification|prompt\s+the\s+user', body, re.IGNORECASE)
    if ask_patterns:
        warnings.append(f"Found {len(ask_patterns)} 'ask the user' pattern(s) — these break autonomous operation. Provide defaults instead.")

    # Check optional directories
    for subdir in ['scripts', 'references', 'assets']:
        subdir_path = os.path.join(skill_dir, subdir)
        if os.path.isdir(subdir_path):
            files = os.listdir(subdir_path)
            if not files:
                warnings.append(f"Empty '{subdir}/' directory — remove if unused")

    return errors, warnings


def main():
    if len(sys.argv) != 2:
        print("Usage: python validate_skill.py path/to/skill-directory")
        sys.exit(1)

    skill_dir = sys.argv[1]
    errors, warnings = validate_skill(skill_dir)

    print(f"\n{'='*50}")
    print(f"  Skill Validation: {os.path.basename(os.path.normpath(skill_dir))}")
    print(f"{'='*50}\n")

    if errors:
        print(f"  ERRORS ({len(errors)}):")
        for e in errors:
            print(f"    ✗ {e}")
        print()

    if warnings:
        print(f"  WARNINGS ({len(warnings)}):")
        for w in warnings:
            print(f"    ⚠ {w}")
        print()

    if not errors and not warnings:
        print("  ✓ PASS — No issues found\n")
    elif not errors:
        print(f"  ✓ PASS with {len(warnings)} warning(s)\n")
    else:
        print(f"  ✗ FAIL — {len(errors)} error(s), {len(warnings)} warning(s)\n")

    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
