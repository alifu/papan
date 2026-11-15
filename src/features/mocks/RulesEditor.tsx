import type { RuleOperator, RuleTarget } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import {
  NEEDS_MODIFIER,
  NO_VALUE_OPERATORS,
  OPERATORS,
  TARGETS,
  newRule,
  removeAt,
  replaceAt,
  type MResponse,
  type MRule,
} from "./mockModel";

export const MAX_RULES = 50;

export interface RulesEditorProps {
  readonly response: MResponse;
  readonly onChange: (response: MResponse) => void;
}

export function RulesEditor({ response, onChange }: RulesEditorProps) {
  const setRule = (index: number, patch: Partial<MRule>) =>
    onChange({
      ...response,
      rules: replaceAt(response.rules, index, {
        ...response.rules[index]!,
        ...patch,
      }),
    });
  return (
    <fieldset className="mock-rules">
      <legend>Rules</legend>
      {response.rules.length > 1 ? (
        <label>
          Match
          <select
            aria-label="Rules operator"
            value={response.rules_operator}
            onChange={(e) =>
              onChange({
                ...response,
                rules_operator: e.target.value === "or" ? "or" : "and",
              })
            }
          >
            <option value="and">all rules (AND)</option>
            <option value="or">any rule (OR)</option>
          </select>
        </label>
      ) : null}
      {response.rules.map((rule, index) => (
        <div key={index} className="mock-row">
          <select
            aria-label={`Rule ${index + 1} target`}
            value={rule.target}
            onChange={(e) =>
              setRule(index, { target: e.target.value as RuleTarget })
            }
          >
            {TARGETS.map((t) => (
              <option key={t.value} value={t.value}>
                {t.label}
              </option>
            ))}
          </select>
          {NEEDS_MODIFIER.has(rule.target) ? (
            <input
              aria-label={`Rule ${index + 1} name`}
              value={rule.modifier}
              placeholder={rule.target === "body" ? "user.role" : "name"}
              onChange={(e) => setRule(index, { modifier: e.target.value })}
            />
          ) : null}
          <select
            aria-label={`Rule ${index + 1} operator`}
            value={rule.operator}
            onChange={(e) =>
              setRule(index, { operator: e.target.value as RuleOperator })
            }
          >
            {OPERATORS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          {NO_VALUE_OPERATORS.has(rule.operator) ? null : (
            <input
              aria-label={`Rule ${index + 1} value`}
              value={rule.value}
              placeholder="Value"
              onChange={(e) => setRule(index, { value: e.target.value })}
            />
          )}
          <label>
            <input
              type="checkbox"
              aria-label={`Rule ${index + 1} invert`}
              checked={rule.invert}
              onChange={(e) => setRule(index, { invert: e.target.checked })}
            />
            not
          </label>
          <Button
            icon="close"
            aria-label={`Remove rule ${index + 1}`}
            onClick={() =>
              onChange({ ...response, rules: removeAt(response.rules, index) })
            }
          />
        </div>
      ))}
      <Button
        icon="plus"
        label="Add rule"
        disabled={response.rules.length >= MAX_RULES}
        onClick={() =>
          onChange({ ...response, rules: [...response.rules, newRule()] })
        }
      />
    </fieldset>
  );
}
