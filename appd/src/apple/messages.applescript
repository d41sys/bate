-- argv: to, text
on run argv
	tell application "Messages"
		set theAccount to first account whose service type = iMessage
		send (item 2 of argv) to participant (item 1 of argv) of theAccount
	end tell
	return "sent"
end run
