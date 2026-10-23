# identity

identity holds every account that acts, `Account { number, card, control }`: a user's (a
person holding their own keys), a program account's (managed by the account that created
it, which suspends, resumes and revokes it and sets the script that runs its actions;
whom it answers is its list in the access program, which harness reads) and each
program's own system account (registered by the kernel as it admits the program).
