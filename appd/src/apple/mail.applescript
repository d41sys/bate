-- argv: action, query, id, to, cc, subject, body
on run argv
	set {action, theQuery, theId, theTo, theCc, theSubject, theBody} to {item 1 of argv, item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv, item 6 of argv, item 7 of argv}
	tell application "Mail"
		if action is "search" then
			set found to messages of inbox
			if theQuery is not "" then set found to (messages of inbox whose subject contains theQuery or sender contains theQuery)
			set out to ""
			repeat with i from 1 to (count of found)
				if i > 25 then exit repeat
				set m to item i of found
				set out to out & my row({id of m, my isoDate(date received of m), sender of m, subject of m})
			end repeat
			return out
		end if
		if action is "read" then
			set m to first message of inbox whose id is (theId as integer)
			return my row({"from", sender of m}) & my row({"date", my isoDate(date received of m)}) & my row({"subject", subject of m}) & linefeed & my clip(content of m, 20000)
		end if
		set m to make new outgoing message with properties {subject:theSubject, content:theBody, visible:(action is "draft")}
		repeat with a in my splitText(theTo, ",")
			if my trim(a as text) is not "" then tell m to make new to recipient at end of to recipients with properties {address:my trim(a as text)}
		end repeat
		repeat with a in my splitText(theCc, ",")
			if my trim(a as text) is not "" then tell m to make new cc recipient at end of cc recipients with properties {address:my trim(a as text)}
		end repeat
		if action is "send" then
			send m
			return "sent"
		end if
		if action is "draft" then return "drafted: the message is open in Mail, unsent"
	end tell
	error "unknown action " & action
end run
