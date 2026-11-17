import type { WsdlImport } from "../../ipc/bindings";

export interface OperationSelection {
  readonly service: string;
  readonly port: string;
  readonly operation: string;
}

export interface OperationListProps {
  readonly wsdl: WsdlImport;
  readonly selected: OperationSelection | null;
  readonly onSelect: (selection: OperationSelection) => void;
}

const isSelected = (s: OperationSelection | null, service: string, port: string, operation: string): boolean =>
  s !== null && s.service === service && s.port === port && s.operation === operation;

/** Service > port (version badge) > operation. Operations are buttons, so Tab and Enter work. */
export function OperationList({ wsdl, selected, onSelect }: OperationListProps) {
  if (wsdl.services.length === 0) return <p className="soap-note">This WSDL has no services.</p>;
  return (
    <ul className="soap-ops" aria-label="WSDL operations">
      {wsdl.services.map((service) => (
        <li key={service.name}>
          <span className="soap-service">{service.name}</span>
          <ul>
            {service.ports.map((port) => (
              <li key={port.name}>
                <span className="soap-port">
                  {port.name} <span className="soap-badge">SOAP {port.version}</span>
                </span>
                <ul>
                  {port.operations.map((op) => (
                    <li key={op.name}>
                      <button
                        type="button"
                        className="soap-op"
                        title={op.documentation || undefined}
                        aria-pressed={isSelected(selected, service.name, port.name, op.name)}
                        onClick={() => onSelect({ service: service.name, port: port.name, operation: op.name })}
                      >
                        {op.name}
                      </button>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}
