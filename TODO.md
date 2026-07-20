Milestones:

  1. Port Havok TYPE-table parsing and rebuilding from PhysicsTool.     DONE!
  2. Port ITEM graph closure collection and PTCH pointer relocation.    DONE!
  3. Port DATA-section rebuilding and reference-array replacement.      DONE!
  4. Port skeleton parsing and cloth/skeleton pairing.                  DONE!
  5. Port AAMP cloth/collidable registration merging.                   DONE!
  6. Implement complete-cloth graph merging with duplicate-name         DONE!
     skipping.

  7. Design and implement standalone Collidable merging—PhysicsTool
     currently only imports colliders as part of a complete cloth
     graph.                                                             DONE!

  8. Add backend APIs to enumerate opened BPHCL documents and           DONE! 
     selectable nodes.

  9. Add Win32 validation message when fewer than two BPHCL documents   DONE!
     are open.

  10. Add the Tools → Physics merge selection subview.                  DONE!
  11. Update disk/archive/nested-archive documents with merged bytes.   DONE!
  12. Validate merged files across the corpus, including reparsing,
     pointer integrity, and archive save/reopen tests.

  The main blocker is that TotkBits currently preserves BPHCL bytes
  but does not rebuild TAG0/TYPE/ITEM/PTCH/DATA sections.
  PhysicsTool’s merge depends on all of those systems and has no
  standalone Collidable merge implementation.