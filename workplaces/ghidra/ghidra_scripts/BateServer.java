// Answers JSON lines on standard input with JSON lines on standard output, so that Bate's
// Ghidra workplace can drive one headless Ghidra. Each request is {"id": n, "op": "...", ...}; the
// answer is "BATE> " and {"id": n, "result": ...} or {"id": n, "error": "..."}. Ghidra's own log
// shares standard output, which is why the answers have the prefix.
//@category Bate

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

import com.google.gson.*;

import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.app.util.importer.ProgramLoader;
import ghidra.app.util.opinion.LoadException;
import ghidra.app.util.opinion.LoadResults;
import ghidra.framework.model.DomainFile;
import ghidra.program.model.address.*;
import ghidra.program.model.data.StringDataInstance;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.util.DefinedDataIterator;

public class BateServer extends GhidraScript {
	private static final Gson GSON = new Gson();
	private static final int MOST_STRINGS = 20000;

	private final Map<String, Program> programs = new HashMap<>();
	private final Map<String, DecompInterface> decompilers = new HashMap<>();

	@Override
	protected void run() throws Exception {
		BufferedReader in =
			new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
		say(Map.of("ready", true));
		String line;
		while ((line = in.readLine()) != null) {
			if (line.isBlank()) {
				continue;
			}
			JsonObject request = JsonParser.parseString(line).getAsJsonObject();
			Map<String, Object> answer = new LinkedHashMap<>();
			answer.put("id", request.get("id"));
			try {
				answer.put("result", handle(request.get("op").getAsString(), request));
			}
			catch (Exception e) {
				answer.put("error", e.getMessage() != null ? e.getMessage() : e.toString());
			}
			say(answer);
		}
		for (DecompInterface d : decompilers.values()) {
			d.dispose();
		}
		for (Program p : programs.values()) {
			p.release(this);
		}
	}

	private void say(Object what) {
		System.out.println("BATE> " + GSON.toJson(what));
		System.out.flush();
	}

	private static String text(JsonObject r, String key) {
		JsonElement e = r.get(key);
		return e == null || e.isJsonNull() ? null : e.getAsString();
	}

	private Object handle(String op, JsonObject r) throws Exception {
		switch (op) {
			case "programs":
				return programNames();
			case "import":
				return importBinary(text(r, "path"));
			case "functions":
				return functions(program(text(r, "program")));
			case "strings":
				return strings(program(text(r, "program")));
			case "imports":
				return imports(program(text(r, "program")));
			case "decompile":
				return decompile(program(text(r, "program")), text(r, "function"));
			case "refs":
				return refs(program(text(r, "program")), text(r, "target"));
			case "rename":
				return rename(program(text(r, "program")), text(r, "function"), text(r, "name"));
			case "comment":
				return comment(program(text(r, "program")), text(r, "function"),
					text(r, "address"), text(r, "text"));
			default:
				throw new IllegalArgumentException("unknown op " + op);
		}
	}

	private Program program(String name) throws Exception {
		Program p = programs.get(name);
		if (p == null) {
			DomainFile file = name == null ? null : getProjectRootFolder().getFile(name);
			if (file == null) {
				throw new IllegalArgumentException("there is no binary " + name);
			}
			p = (Program) file.getDomainObject(this, true, false, monitor);
			programs.put(name, p);
		}
		return p;
	}

	private List<String> programNames() {
		List<String> names = new ArrayList<>();
		for (DomainFile f : getProjectRootFolder().getFiles()) {
			if (Program.class.isAssignableFrom(f.getDomainObjectClass())) {
				names.add(f.getName());
			}
		}
		Collections.sort(names);
		return names;
	}

	private Object importBinary(String path) throws Exception {
		Program p;
		try (LoadResults<Program> loaded = ProgramLoader.builder()
				.source(new File(path)).project(state.getProject()).monitor(monitor).load()) {
			loaded.save(monitor);
			p = loaded.getPrimaryDomainObject(this);
		}
		catch (LoadException e) {
			throw new IllegalArgumentException(
				"Ghidra cannot read " + path + " as a program: " + e.getMessage());
		}
		programs.put(p.getName(), p);
		// With no program of its own, the script is not in a transaction: analysis needs one.
		int tx = p.startTransaction("analyse");
		try {
			analyzeAll(p);
		}
		finally {
			p.endTransaction(tx, true);
		}
		p.save("analysed by Bate", monitor);
		return Map.of("program", p.getName(), "functions",
			p.getFunctionManager().getFunctionCount());
	}

	private static String where(Address a) {
		return a == null ? "" : a.toString();
	}

	private Object functions(Program p) {
		List<Map<String, Object>> out = new ArrayList<>();
		for (Function f : p.getFunctionManager().getFunctions(true)) {
			Map<String, Object> m = new LinkedHashMap<>();
			m.put("address", where(f.getEntryPoint()));
			m.put("name", f.getName());
			m.put("size", f.getBody().getNumAddresses());
			m.put("source", f.getSymbol().getSource().toString());
			out.add(m);
		}
		return out;
	}

	private Object strings(Program p) {
		List<Map<String, Object>> out = new ArrayList<>();
		for (Data d : DefinedDataIterator.byDataInstance(p, StringDataInstance::isString)) {
			StringDataInstance s = StringDataInstance.getStringDataInstance(d);
			out.add(Map.of("address", where(d.getAddress()), "value", s.getStringValue() == null
					? "" : s.getStringValue()));
			if (out.size() >= MOST_STRINGS) {
				break;
			}
		}
		return out;
	}

	private Object imports(Program p) {
		List<Map<String, Object>> out = new ArrayList<>();
		ExternalManager em = p.getExternalManager();
		for (String library : em.getExternalLibraryNames()) {
			ExternalLocationIterator it = em.getExternalLocations(library);
			while (it.hasNext()) {
				ExternalLocation loc = it.next();
				out.add(Map.of("library", library, "name", loc.getLabel(), "address",
					where(loc.getAddress())));
			}
		}
		return out;
	}

	private Function function(Program p, String what) {
		Address a = null;
		try {
			a = p.getAddressFactory().getAddress(what);
		}
		catch (Exception e) {
			// not an address: a name
		}
		if (a != null) {
			Function f = p.getFunctionManager().getFunctionContaining(a);
			if (f != null) {
				return f;
			}
		}
		for (Function f : p.getFunctionManager().getFunctions(true)) {
			if (f.getName().equals(what)) {
				return f;
			}
		}
		throw new IllegalArgumentException("there is no function " + what);
	}

	private List<Map<String, Object>> comments(Program p, Function f) {
		List<Map<String, Object>> out = new ArrayList<>();
		Listing listing = p.getListing();
		AddressIterator it = listing.getCommentAddressIterator(f.getBody(), true);
		while (it.hasNext()) {
			Address a = it.next();
			for (CommentType kind : CommentType.values()) {
				String c = listing.getComment(kind, a);
				if (c != null) {
					out.add(Map.of("address", where(a), "kind", kind.name().toLowerCase(), "text", c));
				}
			}
		}
		return out;
	}

	private Object decompile(Program p, String what) throws Exception {
		Function f = function(p, what);
		DecompInterface ifc = decompilers.get(p.getName());
		if (ifc == null) {
			ifc = new DecompInterface();
			if (!ifc.openProgram(p)) {
				throw new IllegalStateException(
					"the decompiler does not start: " + ifc.getLastMessage());
			}
			decompilers.put(p.getName(), ifc);
		}
		DecompileResults res = ifc.decompileFunction(f, 60, monitor);
		if (!res.decompileCompleted()) {
			throw new IllegalStateException(
				"cannot decompile " + f.getName() + ": " + res.getErrorMessage());
		}
		Map<String, Object> m = new LinkedHashMap<>();
		m.put("address", where(f.getEntryPoint()));
		m.put("name", f.getName());
		m.put("signature", f.getPrototypeString(false, false));
		m.put("code", res.getDecompiledFunction().getC());
		m.put("comments", comments(p, f));
		return m;
	}

	private Object refs(Program p, String what) {
		Address a = null;
		try {
			a = p.getAddressFactory().getAddress(what);
		}
		catch (Exception e) {
			// not an address: a name
		}
		if (a == null) {
			a = function(p, what).getEntryPoint();
		}
		List<Map<String, Object>> out = new ArrayList<>();
		for (Reference ref : p.getReferenceManager().getReferencesTo(a)) {
			Function in = p.getFunctionManager().getFunctionContaining(ref.getFromAddress());
			Map<String, Object> m = new LinkedHashMap<>();
			m.put("from", where(ref.getFromAddress()));
			m.put("function", in == null ? "" : in.getName());
			m.put("type", ref.getReferenceType().getName());
			out.add(m);
		}
		return out;
	}

	private Object rename(Program p, String what, String name) throws Exception {
		Function f = function(p, what);
		String before = f.getName();
		int tx = p.startTransaction("rename " + before);
		try {
			f.setName(name, SourceType.USER_DEFINED);
		}
		finally {
			p.endTransaction(tx, true);
		}
		p.save("renamed by Bate", monitor);
		return Map.of("address", where(f.getEntryPoint()), "was", before, "name", f.getName());
	}

	private Object comment(Program p, String what, String address, String text) throws Exception {
		Function f = function(p, what);
		Address a = address == null ? f.getEntryPoint() : p.getAddressFactory().getAddress(address);
		if (a == null || !f.getBody().contains(a)) {
			throw new IllegalArgumentException(address + " is not in " + f.getName());
		}
		int tx = p.startTransaction("comment " + f.getName());
		try {
			// A comment at the entry heads the function; elsewhere it stands above its line.
			p.getListing().setComment(a,
				a.equals(f.getEntryPoint()) ? CommentType.PLATE : CommentType.PRE,
				text.isEmpty() ? null : text);
		}
		finally {
			p.endTransaction(tx, true);
		}
		p.save("commented by Bate", monitor);
		return Map.of("address", where(a), "function", f.getName());
	}
}
