---
name: airtribe-domain
description: Apply Airtribe domain and lifecycle invariants.
version: 0.1.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, domain, enrollment, cohort, lead, opportunity]
    related_skills: [airtribe-planning, airtribe-evidence-led-builds]
---

# Airtribe Domain Invariants

Use this skill when a mycohort-api task touches learner access, cohort structure, team roles, leads, sales opportunities, mentorship, or registrations. It captures stable semantic constraints; inspect current code for route names, values, and implementation details.

## Entity map

```text
LandingPage → CourseGroup → Course (often called a cohort)
User → Enrollment → Course
Enrollment.parentEnrollmentId → child/sub-course enrollment
Lead → optional User and Course/Workshop
Opportunity → User, optional Lead, LandingPage, AdminUser
TeamMember → course-scoped roles such as Mentor/Instructor
```

## Core distinctions

- A **Lead** is an application/prospect record; a **User** is an authenticated identity; an **Enrollment** grants membership/access. Do not use one as proof that another exists.
- A **Course** is often a runnable cohort but can be another course type. A **CourseGroup** contains courses. “Programme” and “unit” are business terms, not consistently standalone models.
- An operational **AdminUser** is distinct from a User’s course-level `ADMIN` role.
- Learner, mentor, and instructor are contextual course/team roles; do not globally cache a User’s role.
- A sales **Opportunity** can exist with no Lead; status/stage/reason are a coupled state system.
- A payment registration is commercial/reporting provenance. It is not an Enrollment.

## Enrollment and unit rules

- Lead enrollment and actual User Enrollment may be asynchronous; creation must be idempotent and recoverable.
- Child/sub-course enrollment requires a parent cohort enrollment.
- Unit assignment is a complete desired-state replacement: validate requested children, insert missing rows, and revoke omitted rows deliberately.
- A learner may hold only one cohort for a unit. Determine the unit identity from current code, not a display title.
- Cohort transfer is cross-entity work: preserve required ordering, move linked artifacts, serialize competing moves, and acknowledge when a mutex is not a DB transaction.

## Role and permission rules

- Resolve role in the target course/landing-page context.
- Team role precedence can override learner enrollment-derived role.
- Ask whether a request refers to creator/course authorization or CRM/operations authorization before changing permissions.

## Opportunity rules

- Update status and stage through the canonical domain path; validate the pair and stage reason.
- Preserve lifecycle effects such as audit activity, locks, queue events, and linked Lead synchronization.
- Locking/reassignment decisions must recheck authority and state within the transaction; historic lock records may be intentional.

## Clarification rule

If task language says “course,” “cohort,” “unit,” “lead,” “admin,” “status,” “registered,” or “access” without a concrete identifier/entity layer, ask one precise question rather than selecting the most likely table.

## Verification

Every domain-changing plan names the exact entity records and lifecycle transitions it affects, distinguishes commercial state from access state, and lists preservation/rollback behavior for related records.
