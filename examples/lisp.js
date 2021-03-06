import initCore from "lisp_core.js"
function tokenize(s) {
    return s.replace(/\(/g, ' ( ').replace(/\)/g, ' ) ').split(' ').filter((val) => val.length);
}

let float_re = /^[-+]?[0-9]*\.?[0-9]+([eE][-+]?[0-9]+)?$/;
function atom(token) {
    if (float_re.exec(token) !== null)
        return parseFloat(token);
    return token + "";
}


function readFrom(tokens) {
    if (tokens.length === 0)
        throw "Unexpected EOF"

    let token = tokens.shift();
    if (token === '(') {
        let L = [];
        while (tokens[0] !== ')')
            L.push(readFrom(tokens));
        tokens.shift();
        return L;
    }
    else if (token == ')')
        throw "unexpected";
    else
        return atom(token)
}


/*
* Constructs new variable environment.
*/
function Env(params, args, outer) {
    for (let i = 0; i < params.length; i++) {
        this[params[i]] = args[i];
    }
    this.outer = outer;
}

Env.prototype.get = function envGet(name) {
    if (name in this) {
        return this;
    }

    if (this.outer !== undefined) {
        return this.outer.get(name);
    }
    let str = "Variable '" + name + "' not found";

    throw str;
}

const global_env = new Env([], []);
function eval(x, env) {

    if (typeof x === "string") {
        return env.get(x)[x];
    } else if (!Array.isArray(x))
        return x;
    else if (x[0] === 'quote')
        return x[1];
    else if (x[0] === 'if') {
        let test = x[1];
        let conseq = x[2];
        let alt = x[3];
        return eval(eval(test, env) ? conseq : alt, env);
    } else if (x[0] === 'set!') {
        let var_ = x[1];
        let exp = x[2];
        env.get(var_)[var_] = eval(exp, env);
    } else if (x[0] === 'define') {
        let var_ = x[1];
        let exp = x[2];
        env[var_] = eval(exp, env);
    } else if (x[0] === 'lambda') {
        let vars = x[1];
        let exp = x[2];

        return (...args) => {

            let e = new Env(vars, args, env);

            return eval(exp, e);
        }
    } else if (x[0] === 'begin') {
        let val;
        for (let i = 1; i < x.length; i++) {
            val = eval(x[i], env);
        }
        return val;
    } else {
        let exprs = []
        for (let i = 0; i < x.length; i++) {
            exprs[i] = eval(x[i], env)
        }

        proc = exprs.shift();
        return proc(...exprs);
    }
}
function Exit() {

}


function repl() {
    print("Tiny Lisp REPL.\nType '(exit)' to exit from the REPL");
    while (true) {
        try {
            let tokens = tokenize(readLine('> ').trim());
            let ast = readFrom(tokens);
            let val = eval(ast, global_env);
            if (val !== undefined)
                print(val)
        } catch (e) {
            if (e instanceof Exit) {
                print("exit repl");
                return;
            }
            print('Error: ', e);
        }
    }
}
initCore(global_env);
repl();