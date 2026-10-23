# scheduler

scheduler keeps the runs programs schedule for later heights (`Schedule { height,
target, payload }`, answered with the entry id; only the program that scheduled an entry
cancels it, and only before it runs), lists them by height (`Due`), and keeps each run's
receipt, which the kernel hands it (`Ran`) after it runs the entry at the end of its
block as the program that scheduled it (`Cause::Scheduled`)
