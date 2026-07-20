HKCL                                                                                                                                                                                                                                                                                                
  Milestones:                                                                                                                                                                                                                                                                                               
                                                                                                                                                                                                                                                                                                            
  1. Port HKCL binary model and header/section parsing from                DONE! PhysicsTool.                                                                                                                                                                                                                                    
  2. Port HKCL TYPE, ITEM, PTCH, and DATA graph parsing.                 DONE!                                                                                                                                                                                                                               
  3. Port HKCL skeleton, cloth, particle, constraint, and collidable parsing. DONE!                                                                                                                                                                                                                          
  4. Add HKCL graph validation and corpus-based parser tests. DONE!                                                                                                                                                                                                                                         
  5. Add read-only HKCL YAML serialization and document-tree leaves. DONE!                                                                                                                                                                                                                                  
  6. Register .hkcl opening for disk, archive, and nested-archive documents. DONE!                                                                                                                                                                                                                           
  7. Add backend APIs to enumerate opened HKCL documents and selectable nodes. DONE!                                                                                                                                                                                                                        
   8. Port BPHHB header, bone hierarchy, transforms, and metadata parsing. DONE!                                                                                                                                                                                                                             
  9. Add BPHHB validation and corpus-based parser tests. DONE!                                                                                                                                                                                                                                              
  10. Register .bphhb opening and read-only document inspection. DONE!
  11. Define a format-neutral physics graph covering HKCL and BPHCL. DONE!
  12. Port PhysicsTool’s HKCL-to-BPHCL compatibility and conversion rules. DONE!
  13. Port PhysicsTool’s BPHCL-to-HKCL compatibility and conversion rules. DONE!
  14. Implement same-format HKCL complete-cloth and standalone-collidable merging. DONE!
  15. Implement cross-format HKCL → BPHCL merging. DONE!
  16. Implement cross-format BPHCL → HKCL merging. DONE!
  17. Add BPHHB-assisted skeleton/bone mapping where required. DONE!
  18. Extend backend merge APIs with source/target format selection and validation. DONE!
  19. Extend Tools → Physics Merge to support HKCL and BPHCL targets and sources. DONE!
  20. Add read-only HKCL YAML preview UI. DONE!
  21. Update disk, archive, and nested-archive documents with rebuilt HKCL/BPHCL bytes. PARTIAL: validated byte commit and archive propagation implemented; HKCL graph serialization still required.
  22. Validate roundtrips, pointer integrity, conversion behavior, and archive save/reopen across HKCL, BPHCL, and BPHHB corpora. PARTIAL: parser/raw-byte and archive roundtrips, conversion preflights, and rebuilt BPHCL pointer/archive validation are covered; rebuilt HKCL roundtrips await the HKCL serializer from milestone 21.
