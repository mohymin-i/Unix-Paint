// Exports decompiled C for every function plus a symbol/xref index.
// Usage: analyzeHeadless <proj> mspaint -process mspaint.exe -noanalysis \
//          -scriptPath re/tools -postScript ExportDecomp.java <outdir>
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;

import java.io.File;
import java.io.PrintWriter;

public class ExportDecomp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String out = getScriptArgs().length > 0 ? getScriptArgs()[0] : "decomp";
        new File(out).mkdirs();
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        try (PrintWriter all = new PrintWriter(new File(out, "all.c"));
             PrintWriter idx = new PrintWriter(new File(out, "functions.tsv"))) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            int n = 0;
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                if (f.isThunk() || f.isExternal()) continue;
                idx.println(f.getEntryPoint() + "\t" + f.getName(true) + "\t" + f.getBody().getNumAddresses());
                DecompileResults r = d.decompileFunction(f, 60, monitor);
                all.println("// ==== " + f.getEntryPoint() + " " + f.getName(true));
                if (r != null && r.getDecompiledFunction() != null) {
                    all.println(r.getDecompiledFunction().getC());
                } else {
                    all.println("// decompile failed");
                }
                if (++n % 500 == 0) println("decompiled " + n);
            }
            println("total " + n);
        }
        try (PrintWriter sy = new PrintWriter(new File(out, "symbols.tsv"))) {
            SymbolIterator si = currentProgram.getSymbolTable().getAllSymbols(true);
            while (si.hasNext()) {
                Symbol s = si.next();
                sy.println(s.getAddress() + "\t" + s.getSymbolType() + "\t" + s.getName(true));
            }
        }
    }
}
